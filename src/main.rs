// No console window on Windows for the GUI.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod compare;
mod session;
mod shape;
mod types;

use compare::{Finding, Kind};
use session::{Session, Status, Tracked};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

slint::include_modules!();

const REF: usize = 0;
const CAND: usize = 1;
/// Longer lines are cut in the source views.
const MAX_LINE_CHARS: usize = 600;

type Stamp = (SystemTime, u64);

struct App {
    ui: slint::Weak<AppWindow>,
    paths: [String; 2],
    docs: [Option<shape::Document>; 2],
    errors: [String; 2],
    stamps: [Option<Stamp>; 2],
    session: Session,
    rows: Rc<VecModel<Row>>,
    lines: [Rc<VecModel<SharedString>>; 2],
    items: Rc<VecModel<SharedString>>,
    selected_key: Option<String>,
    item: usize,
    last_change: Option<Instant>,
}

thread_local! {
    static APP: RefCell<Option<Rc<RefCell<App>>>> = const { RefCell::new(None) };
}

fn stamp(path: &str) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

fn read_document(path: &str) -> Result<shape::Document, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {path}: {e}"))?;
    shape::parse(&shape::decode(&bytes)).map_err(|e| format!("{path} is not well-formed XML: {e}"))
}

fn display_line(line: &str) -> SharedString {
    let expanded = line.replace('\t', "    ");
    if expanded.chars().count() > MAX_LINE_CHARS {
        let cut: String = expanded.chars().take(MAX_LINE_CHARS).collect();
        format!("{cut} …").into()
    } else {
        expanded.into()
    }
}

/// Last three segments of a path, for the findings list.
fn short_path(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 3 { path.to_string() } else { format!("…/{}", parts[parts.len() - 3..].join("/")) }
}

fn ago(since: Instant) -> String {
    let s = since.elapsed().as_secs();
    match s {
        0..=4 => "just now".into(),
        5..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        _ => format!("{} h ago", s / 3600),
    }
}

impl App {
    fn ui(&self) -> AppWindow {
        self.ui.unwrap()
    }

    /// Chooses a file for one side. A new reference starts a new baseline.
    /// A new candidate is treated like a new version of the previous one, so
    /// its findings are marked new or fixed against the baseline.
    fn set_path(&mut self, side: usize, path: String) {
        let path = path.trim().trim_matches('"').to_string();
        self.paths[side] = path;
        if side == REF {
            self.session.restart();
            self.selected_key = None;
        }
        self.last_change = None;
        self.load(side);
        self.recompute();
        self.refresh();
    }

    fn load(&mut self, side: usize) {
        let path = self.paths[side].clone();
        self.stamps[side] = stamp(&path);
        if path.is_empty() {
            self.docs[side] = None;
            self.errors[side].clear();
        } else {
            match read_document(&path) {
                Ok(doc) => {
                    self.docs[side] = Some(doc);
                    self.errors[side].clear();
                }
                // Keep showing the last good version while the file is broken,
                // for example in the middle of being rewritten.
                Err(e) => self.errors[side] = e,
            }
        }
        let ui = self.ui();
        let lines: Vec<SharedString> = self.docs[side]
            .as_ref()
            .map(|d| d.lines.iter().map(|l| display_line(l)).collect())
            .unwrap_or_default();
        let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as i32;
        self.lines[side].set_vec(lines);
        let name = std::path::Path::new(&self.paths[side])
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if side == REF {
            ui.set_ref_name(name.into());
            ui.set_ref_path(self.paths[side].clone().into());
            ui.set_ref_error(self.errors[side].clone().into());
            ui.set_ref_longest(longest);
        } else {
            ui.set_cand_name(name.into());
            ui.set_cand_path(self.paths[side].clone().into());
            ui.set_cand_error(self.errors[side].clone().into());
            ui.set_cand_longest(longest);
        }
    }

    fn recompute(&mut self) {
        if let (Some(a), Some(b)) = (&self.docs[REF], &self.docs[CAND]) {
            self.session.update(compare::compare(&a.root, &b.root));
        }
    }

    /// Picks up changes on disk. Returns true when something was reloaded.
    fn poll(&mut self) -> bool {
        let mut changed = false;
        for side in [REF, CAND] {
            if !self.paths[side].is_empty() && stamp(&self.paths[side]) != self.stamps[side] {
                self.load(side);
                changed = true;
            }
        }
        if changed {
            self.last_change = Some(Instant::now());
            self.recompute();
            self.refresh();
        } else {
            self.refresh_status();
        }
        changed
    }

    fn visible(&self, t: &Tracked, show_fixed: bool, show_dismissed: bool) -> bool {
        match t.status {
            Status::Fixed => show_fixed,
            _ => show_dismissed || !self.session.is_dismissed(t),
        }
    }

    fn refresh_status(&self) {
        let ui = self.ui();
        let ready = self.docs[REF].is_some() && self.docs[CAND].is_some();
        let text = if !ready {
            String::new()
        } else {
            match self.last_change {
                Some(t) => format!("Watching both files · last change picked up {}", ago(t)),
                None => "Watching both files for changes".to_string(),
            }
        };
        ui.set_watch_status(text.into());
    }

    /// Rebuilds the findings list, the summary and the detail pane.
    fn refresh(&mut self) {
        let ui = self.ui();
        let (show_fixed, show_dismissed) = (ui.get_show_fixed(), ui.get_show_dismissed());
        let mut rows: Vec<Row> = Vec::new();
        for kind in Kind::ALL {
            let members: Vec<(usize, &Tracked)> = self
                .session
                .tracked
                .iter()
                .enumerate()
                .filter(|(_, t)| t.finding.kind == kind && self.visible(t, show_fixed, show_dismissed))
                .collect();
            if members.is_empty() {
                continue;
            }
            rows.push(Row {
                id: -1,
                header: true,
                kind: kind.id(),
                title: kind.label().into(),
                count: members.len() as i32,
                dismissed: self.session.kind_dismissed(kind),
                ..Default::default()
            });
            for (i, t) in members {
                rows.push(Row {
                    id: i as i32,
                    header: false,
                    kind: kind.id(),
                    title: t.finding.title.replace('`', "").into(),
                    subtitle: short_path(&t.finding.path).into(),
                    status: match t.status {
                        Status::Open => 0,
                        Status::New => 1,
                        Status::Fixed => 2,
                    },
                    dismissed: t.status != Status::Fixed && self.session.is_dismissed(t),
                    count: 0,
                    note: t.change.clone().into(),
                });
            }
        }
        self.rows.set_vec(rows);

        let c = self.session.counts();
        let ready = self.docs[REF].is_some() && self.docs[CAND].is_some();
        let summary = if !ready {
            String::new()
        } else if self.session.tracked.is_empty() {
            "The shapes match".to_string()
        } else {
            let mut parts = vec![format!("{} open", c.open + c.new)];
            if c.new > 0 {
                parts.push(format!("{} new", c.new));
            }
            if c.fixed > 0 {
                parts.push(format!("{} fixed", c.fixed));
            }
            if c.dismissed > 0 {
                parts.push(format!("{} dismissed", c.dismissed));
            }
            parts.join(" · ")
        };
        ui.set_summary(summary.into());
        ui.set_has_changes(
            c.new > 0 || c.fixed > 0 || self.session.tracked.iter().any(|t| !t.change.is_empty()),
        );
        self.refresh_status();
        self.show_detail(false);
    }

    fn selected(&self) -> Option<(usize, &Tracked)> {
        let key = self.selected_key.as_ref()?;
        self.session.tracked.iter().enumerate().find(|(_, t)| &t.finding.key == key)
    }

    /// Fills the detail pane for the selected finding; with `jump`, also
    /// scrolls both source views to the current item.
    fn show_detail(&mut self, jump: bool) {
        let ui = self.ui();
        let Some((index, tracked)) = self.selected() else {
            self.selected_key = None;
            ui.set_selected(-1);
            ui.set_ref_line(0);
            ui.set_cand_line(0);
            return;
        };
        let f: &Finding = &tracked.finding;
        let item_index = self.item.min(f.items.len().saturating_sub(1));
        ui.set_selected(index as i32);
        ui.set_detail_kind(f.kind.label().to_uppercase().into());
        ui.set_detail_title(f.title.replace('`', "").into());
        ui.set_detail_path(f.path.clone().into());
        ui.set_detail_summary(f.summary.clone().into());
        ui.set_detail_note(tracked.change.clone().into());
        ui.set_detail_status(match tracked.status {
            Status::Open => 0,
            Status::New => 1,
            Status::Fixed => 2,
        });
        ui.set_detail_dismissed(tracked.status != Status::Fixed && self.session.is_dismissed(tracked));
        let texts: Vec<SharedString> = f.items.iter().map(|i| i.text.replace('`', "").into()).collect();
        ui.set_detail_count(texts.len() as i32);
        ui.set_detail_item(item_index as i32);
        let (ref_line, cand_line) =
            f.items.get(item_index).map(|i| (i.ref_line, i.cand_line)).unwrap_or((None, None));
        self.items.set_vec(texts);
        self.item = item_index;

        let indent = |side: usize, line: Option<u32>| -> i32 {
            line.and_then(|l| self.lines[side].row_data(l as usize - 1))
                .map(|t| t.chars().take_while(|c| c.is_whitespace()).count() as i32)
                .unwrap_or(0)
        };
        ui.set_ref_line(ref_line.unwrap_or(0) as i32);
        ui.set_cand_line(cand_line.unwrap_or(0) as i32);
        if jump {
            let (ref_indent, cand_indent) = (indent(REF, ref_line), indent(CAND, cand_line));
            let weak = self.ui.clone();
            // Scroll after the detail pane has been laid out: its height
            // changes the height of the source views.
            slint::Timer::single_shot(Duration::from_millis(30), move || {
                let Some(ui) = weak.upgrade() else { return };
                if let Some(l) = ref_line {
                    ui.invoke_show_ref(l as i32, ref_indent);
                }
                if let Some(l) = cand_line {
                    ui.invoke_show_cand(l as i32, cand_indent);
                }
            });
        }
    }

    fn select(&mut self, id: i32) {
        if let Some(t) = self.session.tracked.get(id as usize) {
            self.selected_key = Some(t.finding.key.clone());
            self.item = 0;
            self.show_detail(true);
        }
    }

    fn step(&mut self, delta: i32) {
        if let Some((_, t)) = self.selected() {
            let n = t.finding.items.len() as i32;
            if n > 0 {
                self.item = (self.item as i32 + delta).rem_euclid(n) as usize;
                self.show_detail(true);
            }
        }
    }
}

fn text_report(findings: &[Finding]) -> String {
    let mut out = String::new();
    for f in findings {
        out += &format!("[{}] {}\n", f.kind.label(), f.title);
        if !f.path.is_empty() {
            out += &format!("    at {}\n", f.path);
        }
        if !f.summary.is_empty() {
            out += &format!("    {}\n", f.summary);
        }
        for i in &f.items {
            let line = |l: Option<u32>| l.map(|l| l.to_string()).unwrap_or_else(|| "-".into());
            out += &format!(
                "      {} (reference line {}, candidate line {})\n",
                if i.text.is_empty() { "here" } else { &i.text },
                line(i.ref_line),
                line(i.cand_line)
            );
        }
    }
    if findings.is_empty() {
        out += "The shapes match.\n";
    }
    out
}

struct Args {
    files: Vec<String>,
    /// Print findings as text instead of opening the window.
    report: bool,
    report_file: Option<String>,
    /// Development aid: save a picture of the window and quit.
    snapshot: Option<String>,
    select: Option<i32>,
    snapshot_delay: Option<u64>,
    about: bool,
}

fn parse_args() -> Args {
    let mut args =
        Args {
        files: Vec::new(),
        report: false,
        report_file: None,
        snapshot: None,
        select: None,
        snapshot_delay: None,
        about: false,
    };
    let mut it = std::env::args().skip(1).peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--report" => args.report = true,
            "--output" => args.report_file = it.next(),
            "--snapshot" => args.snapshot = it.next(),
            "--select" => args.select = it.next().and_then(|v| v.parse().ok()),
            "--about" => args.about = true,
            "--snapshot-delay" => args.snapshot_delay = it.next().and_then(|v| v.parse().ok()),
            "-h" | "--help" => {
                println!(
                    "Usage: xenomorph [REFERENCE.xml] [CANDIDATE.xml] [--report [--output FILE]]\n\n\
                     Without --report a window opens. With --report the findings are printed\n\
                     (or written to FILE) and the exit code is 1 when there are any."
                );
                std::process::exit(0);
            }
            _ => args.files.push(a),
        }
    }
    args
}

fn run_report(args: &Args) -> ! {
    if args.files.len() != 2 {
        eprintln!("--report needs a reference and a candidate file");
        std::process::exit(2);
    }
    let load = |p: &str| {
        read_document(p).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(2);
        })
    };
    let (a, b) = (load(&args.files[0]), load(&args.files[1]));
    let findings = compare::compare(&a.root, &b.root);
    let text = text_report(&findings);
    match &args.report_file {
        Some(path) => {
            if let Err(e) = std::fs::write(path, text) {
                eprintln!("Cannot write {path}: {e}");
                std::process::exit(2);
            }
        }
        None => print!("{text}"),
    }
    std::process::exit(if findings.is_empty() { 0 } else { 1 });
}

fn with_app(f: impl FnOnce(&mut App)) {
    APP.with(|cell| {
        if let Some(app) = cell.borrow().as_ref() {
            f(&mut app.borrow_mut());
        }
    });
}

fn browse(side: usize) {
    let dialog = move || {
        rfd::FileDialog::new()
            .set_title(if side == REF { "Choose the reference file" } else { "Choose the file to check" })
            .add_filter("XML", &["xml", "xsd", "wsdl", "svg", "xhtml"])
            .add_filter("All files", &["*"])
            .pick_file()
    };
    let apply = move |picked: Option<std::path::PathBuf>| {
        if let Some(p) = picked {
            with_app(|app| app.set_path(side, p.to_string_lossy().into_owned()));
        }
    };
    // macOS requires dialogs on the main thread; elsewhere a helper thread
    // keeps the window responsive while the dialog is open.
    if cfg!(target_os = "macos") {
        apply(dialog());
    } else {
        std::thread::spawn(move || {
            let picked = dialog();
            let _ = slint::invoke_from_event_loop(move || apply(picked));
        });
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let args = parse_args();
    if args.report {
        run_report(&args);
    }

    let ui = AppWindow::new()?;
    ui.set_mono_font(
        if cfg!(windows) {
            "Consolas"
        } else if cfg!(target_os = "macos") {
            "Menlo"
        } else {
            "DejaVu Sans Mono"
        }
        .into(),
    );

    let app = Rc::new(RefCell::new(App {
        ui: ui.as_weak(),
        paths: Default::default(),
        docs: [None, None],
        errors: Default::default(),
        stamps: [None, None],
        session: Session::default(),
        rows: Rc::new(VecModel::default()),
        lines: [Rc::new(VecModel::default()), Rc::new(VecModel::default())],
        items: Rc::new(VecModel::default()),
        selected_key: None,
        item: 0,
        last_change: None,
    }));
    {
        let a = app.borrow();
        ui.set_rows(ModelRc::from(a.rows.clone()));
        ui.set_ref_lines(ModelRc::from(a.lines[REF].clone()));
        ui.set_cand_lines(ModelRc::from(a.lines[CAND].clone()));
        ui.set_detail_items(ModelRc::from(a.items.clone()));
    }
    APP.with(|cell| *cell.borrow_mut() = Some(app.clone()));

    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    ui.on_open_url(|url| {
        if let Err(e) = open::that(url.as_str()) {
            eprintln!("Cannot open {url}: {e}");
        }
    });
    ui.on_browse(|side| browse(side as usize));
    ui.on_path_accepted(|side, path| with_app(|app| app.set_path(side as usize, path.to_string())));
    ui.on_select(|id| with_app(|app| app.select(id)));
    ui.on_step(|delta| with_app(|app| app.step(delta)));
    ui.on_select_item(|i| {
        with_app(|app| {
            app.item = i.max(0) as usize;
            app.show_detail(true);
        })
    });
    ui.on_dismiss(|id| {
        with_app(|app| {
            if let Some(key) = app.session.tracked.get(id as usize).map(|t| t.finding.key.clone()) {
                app.session.toggle_finding(&key);
                app.refresh();
            }
        })
    });
    ui.on_dismiss_kind(|kind| {
        with_app(|app| {
            if let Some(kind) = Kind::from_id(kind) {
                app.session.toggle_kind(kind);
                app.refresh();
            }
        })
    });
    ui.on_filters_changed(|| with_app(|app| app.refresh()));
    ui.on_recheck(|| {
        with_app(|app| {
            app.load(REF);
            app.load(CAND);
            app.recompute();
            app.refresh();
        })
    });
    ui.on_accept_baseline(|| {
        with_app(|app| {
            app.session.accept_baseline();
            app.refresh();
        })
    });

    ui.set_about_open(args.about);
    for (side, path) in args.files.iter().take(2).enumerate() {
        app.borrow_mut().set_path(side, path.clone());
    }

    // Re-check when either file changes on disk.
    let watcher = slint::Timer::default();
    watcher.start(slint::TimerMode::Repeated, Duration::from_secs(1), || {
        with_app(|app| {
            app.poll();
        })
    });

    let snapshot_timer = slint::Timer::default();
    if let Some(path) = args.snapshot.clone() {
        let weak = ui.as_weak();
        let select = args.select;
        let next_candidate = args.files.get(2).cloned();
        let step = Rc::new(RefCell::new(0));
        let delay = Duration::from_millis(args.snapshot_delay.unwrap_or(700));
        snapshot_timer.start(slint::TimerMode::Repeated, delay, move || {
            let n = *step.borrow();
            *step.borrow_mut() += 1;
            if n == 0 {
                // A third file is loaded as the next candidate first.
                if let Some(next) = next_candidate.clone() {
                    with_app(|app| app.set_path(CAND, next));
                }
                if let Some(id) = select {
                    with_app(|app| app.select(id));
                }
                return;
            }
            let ui = weak.unwrap();
            match ui.window().take_snapshot() {
                Ok(pixels) => {
                    let saved = image::save_buffer(
                        &path,
                        pixels.as_bytes(),
                        pixels.width(),
                        pixels.height(),
                        image::ColorType::Rgba8,
                    );
                    if let Err(e) = saved {
                        eprintln!("Cannot save snapshot: {e}");
                    }
                }
                Err(e) => eprintln!("Cannot take snapshot: {e}"),
            }
            let _ = slint::quit_event_loop();
        });
    }

    ui.run()
}
