//! Compares the shape of a candidate document against a reference and
//! reports grouped findings.

use crate::shape::ShapeNode;
use crate::types::{diff_profiles, shorten};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Namespace,
    Misnamed,
    Misplaced,
    Structure,
    Missing,
    Empty,
    Unexpected,
    Order,
    Repetition,
    Incomplete,
    Attributes,
    Format,
    Unset,
}

impl Kind {
    pub const ALL: [Kind; 13] = [
        Kind::Namespace,
        Kind::Misnamed,
        Kind::Misplaced,
        Kind::Structure,
        Kind::Missing,
        Kind::Empty,
        Kind::Unexpected,
        Kind::Order,
        Kind::Repetition,
        Kind::Incomplete,
        Kind::Attributes,
        Kind::Format,
        Kind::Unset,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Namespace => "Wrong namespace",
            Kind::Misnamed => "Probably misnamed",
            Kind::Misplaced => "Wrong nesting level",
            Kind::Structure => "Text instead of child elements",
            Kind::Missing => "Missing elements",
            Kind::Empty => "Empty in candidate",
            Kind::Unexpected => "Unexpected elements",
            Kind::Order => "Element order",
            Kind::Repetition => "Repeated where reference has one",
            Kind::Incomplete => "Not in every entry",
            Kind::Attributes => "Attributes",
            Kind::Format => "Value format",
            Kind::Unset => "Unset-looking values",
        }
    }

    pub fn id(self) -> i32 {
        Kind::ALL.iter().position(|k| *k == self).unwrap() as i32
    }

    pub fn from_id(id: i32) -> Option<Kind> {
        Kind::ALL.get(id as usize).copied()
    }
}

/// One place a finding points at. Either line may be absent (for example a
/// missing element has no line of its own in the candidate; then the parent
/// is used where possible).
#[derive(Clone, Debug)]
pub struct Item {
    /// Stable identity inside the finding, used to tell what changed between runs.
    pub id: String,
    pub text: String,
    pub ref_line: Option<u32>,
    pub cand_line: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Finding {
    /// Stable identity across runs.
    pub key: String,
    pub kind: Kind,
    pub title: String,
    /// Path of the element the finding is about (candidate naming).
    pub path: String,
    pub summary: String,
    pub items: Vec<Item>,
    sort_line: u32,
}

struct Registered {
    local: String,
    parent: String,
    finding: usize,
    item: usize,
}

#[derive(Default)]
struct Ctx {
    findings: Vec<Finding>,
    missing: Vec<Registered>,
    unexpected: Vec<Registered>,
    /// Format differences grouped by category.
    formats: BTreeMap<&'static str, (&'static str, Vec<Item>)>,
}

pub fn compare(reference: &ShapeNode, candidate: &ShapeNode) -> Vec<Finding> {
    let mut ctx = Ctx::default();
    let mut path = vec![candidate.local.clone()];
    if reference.local != candidate.local {
        ctx.findings.push(Finding {
            key: "root-name".into(),
            kind: Kind::Misnamed,
            title: format!("Root element is `{}`, reference has `{}`", candidate.local, reference.local),
            path: candidate.local.clone(),
            summary: "The documents have different root elements; the comparison below may not be meaningful."
                .into(),
            items: vec![item("root", String::new(), reference.first_line(), candidate.first_line())],
            sort_line: 0,
        });
    } else if reference.ns != candidate.ns {
        ctx.findings.push(namespace_finding(reference, candidate, &path));
    }
    compare_nodes(reference, candidate, &mut path, &mut ctx);
    cross_reference(&mut ctx);

    for (category, (heading, mut items)) in std::mem::take(&mut ctx.formats) {
        items.sort_by_key(|i| i.cand_line);
        let kind = if category == "special" { Kind::Unset } else { Kind::Format };
        let n = items.len();
        ctx.findings.push(Finding {
            key: format!("format|{category}"),
            kind,
            title: format!("{heading}: {n} element{}", plural(n)),
            path: String::new(),
            summary: format_summary(category).into(),
            sort_line: items.first().and_then(|i| i.cand_line).unwrap_or(0),
            items,
        });
    }

    ctx.findings.sort_by_key(|f| (f.kind, f.sort_line));
    ctx.findings
}

fn format_summary(category: &str) -> &'static str {
    match category {
        "special" => "These values look like a missing value that was written out literally.",
        "num-grouping" => "Numbers are written with a thousands separator in one file and without in the other.",
        "num-separator" => "The decimal separator differs.",
        "num-scientific" => "Scientific notation (for example 1.0E-4) appears only in the candidate.",
        "num-int-dec" => "The reference writes whole numbers without a fraction; the candidate always writes a fraction.",
        "num-precision" => "The number of fraction digits differs. This can also mean a different unit, such as 0.035 against 3.5 percent.",
        "date-format" => "Dates are written in a different pattern.",
        "timezone" => "One file writes a time zone and the other does not, or they write it differently.",
        "time-precision" => "Timestamps have a different number of fractional-second digits.",
        _ => "The kind of value differs.",
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn item(id: impl Into<String>, text: String, ref_line: Option<u32>, cand_line: Option<u32>) -> Item {
    Item { id: id.into(), text, ref_line, cand_line }
}

fn join(path: &[String]) -> String {
    path.join("/")
}

/// Last segments of a path, for titles.
fn tail(path: &[String], n: usize) -> String {
    let start = path.len().saturating_sub(n);
    path[start..].join("/")
}

fn namespace_finding(a: &ShapeNode, b: &ShapeNode, path: &[String]) -> Finding {
    Finding {
        key: format!("namespace|{}", join(path)),
        kind: Kind::Namespace,
        title: format!("`{}` is in the wrong namespace", tail(path, 2)),
        path: join(path),
        summary: format!(
            "Candidate writes `{}` ({}); reference writes `{}` ({}).",
            b.qname(),
            b.ns_label(),
            a.qname(),
            a.ns_label()
        ),
        items: vec![item("ns", String::new(), a.first_line(), b.first_line())],
        sort_line: b.first_line().unwrap_or(0),
    }
}

fn name_similarity(a: &str, b: &str) -> f64 {
    let (la, lb) = (a.to_lowercase(), b.to_lowercase());
    if la == lb { 1.0 } else { strsim::normalized_levenshtein(&la, &lb) }
}

fn child_overlap(a: &ShapeNode, b: &ShapeNode) -> f64 {
    let names = |n: &ShapeNode| n.children.iter().map(|c| c.local.to_lowercase()).collect::<Vec<_>>();
    let (na, nb) = (names(a), names(b));
    let common = na.iter().filter(|x| nb.contains(x)).count();
    let union = na.len() + nb.len() - common;
    if union == 0 { 0.0 } else { common as f64 / union as f64 }
}

/// Score in 0..=1 when `b` looks like a misnamed `a`, otherwise None.
fn rename_score(a: &ShapeNode, b: &ShapeNode) -> Option<f64> {
    let name = name_similarity(&a.local, &b.local);
    match (a.is_container(), b.is_container()) {
        (true, true) => {
            let score = 0.6 * name + 0.4 * child_overlap(a, b);
            (name >= 0.75 || (score >= 0.7 && name >= 0.5)).then_some(score.max(name))
        }
        (false, false) => (name >= 0.75).then_some(name),
        _ => (name >= 0.85).then_some(name),
    }
}

/// Explains where two similar names differ.
fn describe_rename(yours: &str, reference: &str) -> String {
    if yours.to_lowercase() == reference.to_lowercase() {
        return "Only upper and lower case differ.".into();
    }
    let (y, r): (Vec<char>, Vec<char>) = (yours.chars().collect(), reference.chars().collect());
    let mut p = 0;
    while p < y.len() && p < r.len() && y[p] == r[p] {
        p += 1;
    }
    let mut s = 0;
    while s < y.len() - p && s < r.len() - p && y[y.len() - 1 - s] == r[r.len() - 1 - s] {
        s += 1;
    }
    let mid = |v: &[char]| v[p..v.len() - s].iter().collect::<String>();
    let (my, mr) = (mid(&y), mid(&r));
    let before: String = y[p.saturating_sub(8)..p].iter().collect();
    let after: String = y[y.len() - s..(y.len() - s + 6).min(y.len())].iter().collect();
    let lead = if p > 8 { "…" } else { "" };
    let trail = if s > 6 { "…" } else { "" };
    let what = if my.is_empty() {
        format!("“{mr}” is missing")
    } else if mr.is_empty() {
        format!("“{my}” is extra")
    } else {
        format!("“{my}” should be “{mr}”")
    };
    format!("{what}: {lead}{before}[{my}]{after}{trail} → {lead}{before}[{mr}]{after}{trail}")
}

fn strip_digits(s: &str) -> &str {
    s.trim_end_matches(|c: char| c.is_ascii_digit())
}

/// Finds a descendant of `node` with the given local name, at most `depth` levels down.
fn find_descendant<'a>(node: &'a ShapeNode, local: &str, depth: usize, trail: &mut Vec<String>) -> Option<&'a ShapeNode> {
    for c in &node.children {
        trail.push(c.local.clone());
        if c.local == local {
            return Some(c);
        }
        if depth > 1 {
            if let Some(found) = find_descendant(c, local, depth - 1, trail) {
                return Some(found);
            }
        }
        trail.pop();
    }
    None
}

/// Orders the given children by how they usually follow each other.
fn usual_order(node: &ShapeNode, idxs: &[usize]) -> Vec<usize> {
    let before = |x: usize, y: usize| node.before.get(&(x, y)).copied().unwrap_or(0);
    let mut scored: Vec<(usize, usize)> = idxs
        .iter()
        .map(|&x| (idxs.iter().filter(|&&y| y != x && before(x, y) > before(y, x)).count(), x))
        .collect();
    scored.sort_by(|l, r| r.0.cmp(&l.0).then(l.1.cmp(&r.1)));
    scored.into_iter().map(|(_, x)| x).collect()
}

/// Elements of `seq` that are part of a longest common subsequence with `other`.
fn lcs_members(seq: &[usize], other: &[usize]) -> Vec<bool> {
    let (n, m) = (seq.len(), other.len());
    let mut t = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i][j] = if seq[i] == other[j] { t[i + 1][j + 1] + 1 } else { t[i + 1][j].max(t[i][j + 1]) };
        }
    }
    let mut keep = vec![false; n];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if seq[i] == other[j] {
            keep[i] = true;
            i += 1;
            j += 1;
        } else if t[i + 1][j] >= t[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    keep
}

/// Line of the first occurrence of `parent` that does not contain `child`.
fn first_parent_without(parent: &ShapeNode, child: &ShapeNode) -> Option<u32> {
    for (i, start) in parent.lines.iter().enumerate() {
        let end = parent.lines.get(i + 1).copied().unwrap_or(u32::MAX);
        if !child.lines.iter().any(|l| *l >= *start && *l < end) {
            return Some(*start);
        }
    }
    parent.first_line()
}

/// Detects a wrapper level that exists in only one of the documents and
/// queues the wrapped elements for comparison.
#[allow(clippy::too_many_arguments)]
fn detect_wrappers<'n>(
    a: &'n ShapeNode,
    b: &'n ShapeNode,
    un_a: &mut Vec<usize>,
    un_b: &mut Vec<usize>,
    here: &str,
    fuzzy: bool,
    ctx: &mut Ctx,
    nested: &mut Vec<(&'n ShapeNode, &'n ShapeNode, Vec<String>)>,
) {
    // Wrapper element missing in the candidate: reference P/W/x, candidate P/x.
    for ai in un_a.clone() {
        let wrapper = &a.children[ai];
        if !wrapper.is_container() {
            continue;
        }
        let hit = un_b.iter().copied().find_map(|bi| {
            let bc = &b.children[bi];
            wrapper
                .children
                .iter()
                .find(|wc| wc.local == bc.local || (fuzzy && rename_score(wc, bc).is_some_and(|s| s >= 0.8)))
                .map(|wc| (bi, wc))
        });
        if let Some((bi, inner)) = hit {
            let bc = &b.children[bi];
            un_a.retain(|x| *x != ai);
            un_b.retain(|x| *x != bi);
            ctx.findings.push(Finding {
                key: format!("wrapper-missing|{here}/{}", wrapper.local),
                kind: Kind::Misplaced,
                title: format!("`{}` should be wrapped in `{}`", bc.local, wrapper.local),
                path: format!("{here}/{}", bc.local),
                summary: format!(
                    "Reference nests it as `{}/{}/{}`; candidate has `{}/{}`.",
                    a.local, wrapper.local, inner.local, b.local, bc.local
                ),
                items: vec![item("w", String::new(), wrapper.first_line(), bc.first_line())],
                sort_line: bc.first_line().unwrap_or(0),
            });
            nested.push((inner, bc, vec![bc.local.clone()]));
        }
    }
    // Extra wrapper in the candidate: reference P/x, candidate P/W/x.
    for bi in un_b.clone() {
        let wrapper = &b.children[bi];
        if !wrapper.is_container() {
            continue;
        }
        let hit = un_a.iter().copied().find_map(|ai| {
            let ac = &a.children[ai];
            wrapper
                .children
                .iter()
                .find(|wc| wc.local == ac.local || (fuzzy && rename_score(ac, wc).is_some_and(|s| s >= 0.8)))
                .map(|wc| (ai, wc))
        });
        if let Some((ai, inner)) = hit {
            let ac = &a.children[ai];
            un_a.retain(|x| *x != ai);
            un_b.retain(|x| *x != bi);
            ctx.findings.push(Finding {
                key: format!("wrapper-extra|{here}/{}", wrapper.local),
                kind: Kind::Misplaced,
                title: format!("`{}` is wrapped in an extra `{}`", inner.local, wrapper.local),
                path: format!("{here}/{}", wrapper.local),
                summary: format!(
                    "Reference has `{}/{}` directly; candidate has `{}/{}/{}`.",
                    a.local, ac.local, b.local, wrapper.local, inner.local
                ),
                items: vec![item("w", String::new(), ac.first_line(), wrapper.first_line())],
                sort_line: wrapper.first_line().unwrap_or(0),
            });
            nested.push((ac, inner, vec![wrapper.local.clone(), inner.local.clone()]));
        }
    }
}

fn compare_nodes(a: &ShapeNode, b: &ShapeNode, path: &mut Vec<String>, ctx: &mut Ctx) {
    let here = join(path);
    let b_line = b.first_line();
    let a_line = a.first_line();

    // --- text against child elements ---------------------------------------
    if a.with_children > 0 && b.with_children == 0 && b.with_text > 0 {
        ctx.findings.push(Finding {
            key: format!("structure|{here}"),
            kind: Kind::Structure,
            title: format!("`{}` has text, reference has child elements", tail(path, 2)),
            path: here.clone(),
            summary: format!("Reference `{}` contains {} child elements.", a.local, a.children.len()),
            items: vec![item("s", String::new(), a_line, b_line)],
            sort_line: b_line.unwrap_or(0),
        });
        return;
    }
    if a.with_children == 0 && a.with_text > 0 && b.with_children > 0 {
        ctx.findings.push(Finding {
            key: format!("structure|{here}"),
            kind: Kind::Structure,
            title: format!("`{}` has child elements, reference has text", tail(path, 2)),
            path: here.clone(),
            summary: format!("Reference `{}` is a plain value.", a.local),
            items: vec![item("s", String::new(), a_line, b_line)],
            sort_line: b_line.unwrap_or(0),
        });
        return;
    }

    // --- attributes ---------------------------------------------------------
    let mut attr_items = Vec::new();
    for name in a.attrs.keys().filter(|n| !b.attrs.contains_key(*n)) {
        attr_items.push(item(format!("-{name}"), format!("`{name}` is missing"), a_line, b_line));
    }
    for name in b.attrs.keys().filter(|n| !a.attrs.contains_key(*n)) {
        attr_items.push(item(format!("+{name}"), format!("`{name}` is not in the reference"), a_line, b_line));
    }
    if !attr_items.is_empty() {
        ctx.findings.push(Finding {
            key: format!("attributes|{here}"),
            kind: Kind::Attributes,
            title: format!("Attributes differ on `{}`", tail(path, 2)),
            path: here.clone(),
            summary: String::new(),
            items: attr_items,
            sort_line: b_line.unwrap_or(0),
        });
    }

    // --- value format (leaves only) ----------------------------------------
    if !a.is_container() && !b.is_container() {
        for note in diff_profiles(&a.values, &b.values) {
            let entry = ctx.formats.entry(note.category).or_insert((note.heading, Vec::new()));
            entry.1.push(item(
                here.clone(),
                format!("`{}`: {}", tail(path, 2), note.message),
                note.line_ref.or(a_line),
                note.line_cand.or(b_line),
            ));
        }
        return;
    }

    // --- match children -----------------------------------------------------
    // (reference child, candidate child)
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut un_a: Vec<usize> = Vec::new();
    let mut used_b = vec![false; b.children.len()];
    for (ai, ac) in a.children.iter().enumerate() {
        match b.child(&ac.ns, &ac.local) {
            Some(bi) => {
                pairs.push((ai, bi));
                used_b[bi] = true;
            }
            None => un_a.push(ai),
        }
    }
    let mut un_b: Vec<usize> = (0..b.children.len()).filter(|i| !used_b[*i]).collect();

    // Same local name in another namespace.
    un_a.retain(|&ai| {
        let ac = &a.children[ai];
        match un_b.iter().position(|&bi| b.children[bi].local == ac.local) {
            Some(pos) => {
                let bi = un_b.remove(pos);
                path.push(b.children[bi].local.clone());
                ctx.findings.push(namespace_finding(ac, &b.children[bi], path));
                path.pop();
                pairs.push((ai, bi));
                false
            }
            None => true,
        }
    });

    // A missing or extra wrapper level, with exactly matching names. This runs
    // before the name matching so that `entry` is not taken for a misspelt
    // `entryList`.
    // (reference node, candidate node, candidate path segments below `here`)
    let mut nested: Vec<(&ShapeNode, &ShapeNode, Vec<String>)> = Vec::new();
    detect_wrappers(a, b, &mut un_a, &mut un_b, &here, false, ctx, &mut nested);

    // Similar names: best matches first.
    let mut candidates: Vec<(f64, usize, usize)> = Vec::new();
    for &ai in &un_a {
        for &bi in &un_b {
            if let Some(score) = rename_score(&a.children[ai], &b.children[bi]) {
                candidates.push((score, ai, bi));
            }
        }
    }
    candidates.sort_by(|l, r| r.0.partial_cmp(&l.0).unwrap().then(l.1.cmp(&r.1)));
    for (score, ai, bi) in candidates {
        if !un_a.contains(&ai) || !un_b.contains(&bi) {
            continue;
        }
        un_a.retain(|x| *x != ai);
        un_b.retain(|x| *x != bi);
        let (ac, bc) = (&a.children[ai], &b.children[bi]);
        ctx.findings.push(Finding {
            key: format!("misnamed|{here}/{}", ac.local),
            kind: Kind::Misnamed,
            title: format!("`{}` should probably be `{}`", bc.local, ac.local),
            path: format!("{here}/{}", bc.local),
            summary: format!(
                "{} Name similarity {:.0} %.{}",
                describe_rename(&bc.local, &ac.local),
                score * 100.0,
                if ac.ns != bc.ns {
                    format!(" The namespace also differs: {} against {}.", bc.ns_label(), ac.ns_label())
                } else {
                    String::new()
                }
            ),
            items: vec![item("r", String::new(), ac.first_line(), bc.first_line())],
            sort_line: bc.first_line().unwrap_or(0),
        });
        pairs.push((ai, bi));
    }

    // Wrappers again, now also accepting similar names.
    detect_wrappers(a, b, &mut un_a, &mut un_b, &here, true, ctx, &mut nested);

    // Candidate elements that exist deeper inside a missing reference element
    // (also covers numbered variants such as value1, value2 → list/entry/value).
    let mut deeper: BTreeMap<String, (u32, Vec<Item>)> = BTreeMap::new();
    un_b.retain(|&bi| {
        let bc = &b.children[bi];
        let wanted = strip_digits(&bc.local);
        if wanted.is_empty() {
            return true;
        }
        for &ai in &un_a {
            let mut trail = vec![a.children[ai].local.clone()];
            if let Some(target) = find_descendant(&a.children[ai], wanted, 3, &mut trail) {
                let entry = deeper.entry(trail.join("/")).or_insert((bc.first_line().unwrap_or(0), Vec::new()));
                entry.1.push(item(
                    bc.local.clone(),
                    format!("`{}`", bc.local),
                    target.first_line(),
                    bc.first_line(),
                ));
                return false;
            }
        }
        true
    });
    for (target, (line, items)) in deeper {
        let names = items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>();
        ctx.findings.push(Finding {
            key: format!("deeper|{here}/{target}"),
            kind: Kind::Misplaced,
            title: if names.len() == 1 {
                format!("`{}` belongs deeper, at `{}`", names[0], target)
            } else {
                format!("{} elements belong deeper, at `{}`", names.len(), target)
            },
            path: here.clone(),
            summary: format!("Reference has this value nested as `{}/{}`.", a.local, target),
            items,
            sort_line: line,
        });
    }

    // --- missing ------------------------------------------------------------
    if !un_a.is_empty() {
        let empty = b.children.is_empty();
        let finding_idx = ctx.findings.len();
        let mut items = Vec::new();
        for &ai in &un_a {
            let ac = &a.children[ai];
            let mut text = format!("`{}`", ac.local);
            let nested_count = ac.descendant_count();
            if nested_count > 0 {
                text += &format!(" (with {nested_count} nested element{})", plural(nested_count));
            }
            if ac.present_in < a.count {
                text += &format!(" — optional in reference: in {} of {}", ac.present_in, a.count);
            }
            ctx.missing.push(Registered {
                local: ac.local.clone(),
                parent: tail(path, 2),
                finding: finding_idx,
                item: items.len(),
            });
            items.push(item(ac.local.clone(), text, ac.first_line(), b_line));
        }
        let n = items.len();
        let (kind, title) = if empty {
            (
                Kind::Empty,
                format!("`{}` is empty, reference has {} child element{}", tail(path, 2), n, plural(n)),
            )
        } else if n == 1 {
            (Kind::Missing, format!("`{}` is missing in `{}`", a.children[un_a[0]].local, tail(path, 2)))
        } else {
            (Kind::Missing, format!("{} elements missing in `{}`", n, tail(path, 2)))
        };
        ctx.findings.push(Finding {
            key: format!("missing|{here}"),
            kind,
            title,
            path: here.clone(),
            summary: String::new(),
            items,
            sort_line: b_line.unwrap_or(0),
        });
    }

    // --- unexpected ---------------------------------------------------------
    if !un_b.is_empty() {
        let finding_idx = ctx.findings.len();
        let mut items = Vec::new();
        for &bi in &un_b {
            let bc = &b.children[bi];
            let mut text = format!("`{}`", bc.local);
            let nested_count = bc.descendant_count();
            if nested_count > 0 {
                text += &format!(" (with {nested_count} nested element{})", plural(nested_count));
            }
            ctx.unexpected.push(Registered {
                local: bc.local.clone(),
                parent: tail(path, 2),
                finding: finding_idx,
                item: items.len(),
            });
            items.push(item(bc.local.clone(), text, a_line, bc.first_line()));
        }
        let n = items.len();
        ctx.findings.push(Finding {
            key: format!("unexpected|{here}"),
            kind: Kind::Unexpected,
            title: if n == 1 {
                format!("`{}` is not in the reference `{}`", b.children[un_b[0]].local, tail(path, 2))
            } else {
                format!("{} elements in `{}` are not in the reference", n, tail(path, 2))
            },
            path: here.clone(),
            summary: if a.children.is_empty() {
                format!("Reference `{}` is always empty.", a.local)
            } else {
                String::new()
            },
            sort_line: items.first().and_then(|i| i.cand_line).unwrap_or(0),
            items,
        });
    }

    // --- order --------------------------------------------------------------
    if pairs.len() >= 2 {
        let a_order = usual_order(a, &pairs.iter().map(|p| p.0).collect::<Vec<_>>());
        let b_order = usual_order(b, &pairs.iter().map(|p| p.1).collect::<Vec<_>>());
        // Express both orders as indexes into `pairs`.
        let seq_a: Vec<usize> = a_order.iter().map(|ai| pairs.iter().position(|p| p.0 == *ai).unwrap()).collect();
        let seq_b: Vec<usize> = b_order.iter().map(|bi| pairs.iter().position(|p| p.1 == *bi).unwrap()).collect();
        if seq_a != seq_b {
            let keep = lcs_members(&seq_b, &seq_a);
            let mut items = Vec::new();
            for (pos, &p) in seq_b.iter().enumerate() {
                if keep[pos] {
                    continue;
                }
                let (ai, bi) = pairs[p];
                let at = seq_a.iter().position(|x| *x == p).unwrap();
                let place = if at == 0 {
                    "should come first".to_string()
                } else {
                    format!("should come after `{}`", a.children[pairs[seq_a[at - 1]].0].local)
                };
                items.push(item(
                    b.children[bi].local.clone(),
                    format!("`{}` {}", b.children[bi].local, place),
                    a.children[ai].first_line(),
                    b.children[bi].first_line(),
                ));
            }
            let names = |seq: &[usize], cand: bool| {
                seq.iter()
                    .map(|p| if cand { b.children[pairs[*p].1].local.as_str() } else { a.children[pairs[*p].0].local.as_str() })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            ctx.findings.push(Finding {
                key: format!("order|{here}"),
                kind: Kind::Order,
                title: format!("Order of {} element{} differs in `{}`", items.len(), plural(items.len()), tail(path, 2)),
                path: here.clone(),
                summary: format!(
                    "Reference order: {}. Candidate order: {}.",
                    shorten(&names(&seq_a, false), 400),
                    shorten(&names(&seq_b, true), 400)
                ),
                items,
                sort_line: b_line.unwrap_or(0),
            });
        }
    }

    // --- repetition and completeness ---------------------------------------
    let mut repeated = Vec::new();
    let mut incomplete = Vec::new();
    for &(ai, bi) in &pairs {
        let (ac, bc) = (&a.children[ai], &b.children[bi]);
        if ac.max_per_parent == 1 && bc.max_per_parent > 1 {
            repeated.push(item(
                bc.local.clone(),
                format!("`{}` occurs up to {}× per `{}`", bc.local, bc.max_per_parent, b.local),
                ac.first_line(),
                bc.lines.get(1).copied().or(bc.first_line()),
            ));
        }
        if b.count >= 2 && ac.present_in == a.count && bc.present_in < b.count {
            incomplete.push(item(
                bc.local.clone(),
                format!("`{}` is in {} of {}", bc.local, bc.present_in, b.count),
                ac.first_line(),
                first_parent_without(b, bc),
            ));
        }
    }
    if !repeated.is_empty() {
        ctx.findings.push(Finding {
            key: format!("repetition|{here}"),
            kind: Kind::Repetition,
            title: format!("{} element{} repeated in `{}`", repeated.len(), plural(repeated.len()), tail(path, 2)),
            path: here.clone(),
            summary: "The reference has each of these exactly once per parent.".into(),
            sort_line: repeated[0].cand_line.unwrap_or(0),
            items: repeated,
        });
    }
    if !incomplete.is_empty() {
        ctx.findings.push(Finding {
            key: format!("incomplete|{here}"),
            kind: Kind::Incomplete,
            title: format!(
                "{} element{} not in every `{}`",
                incomplete.len(),
                plural(incomplete.len()),
                tail(path, 1)
            ),
            path: here.clone(),
            summary: format!(
                "The reference has these in every one of its {} `{}` entries; the candidate has {} entries.",
                a.count, a.local, b.count
            ),
            sort_line: incomplete[0].cand_line.unwrap_or(0),
            items: incomplete,
        });
    }

    // --- recurse ------------------------------------------------------------
    pairs.sort_by_key(|p| p.1);
    for (ai, bi) in pairs {
        path.push(b.children[bi].local.clone());
        compare_nodes(&a.children[ai], &b.children[bi], path, ctx);
        path.pop();
    }
    for (ac, bc, segments) in nested {
        let depth = segments.len();
        path.extend(segments);
        compare_nodes(ac, bc, path, ctx);
        path.truncate(path.len() - depth);
    }
}

/// Notes on missing and unexpected elements that have the same name in
/// different places: the element is probably just in the wrong parent.
fn cross_reference(ctx: &mut Ctx) {
    let mut notes: Vec<(usize, usize, String)> = Vec::new();
    for u in &ctx.unexpected {
        if let Some(m) = ctx.missing.iter().find(|m| m.local == u.local && m.parent != u.parent) {
            notes.push((u.finding, u.item, format!(" — the reference has this name under `{}`", m.parent)));
            notes.push((m.finding, m.item, format!(" — the candidate has this name under `{}`", u.parent)));
        }
    }
    notes.dedup();
    for (f, i, text) in notes {
        let it = &mut ctx.findings[f].items[i];
        if !it.text.ends_with(&text) {
            it.text += &text;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::parse;

    fn run(reference: &str, candidate: &str) -> Vec<Finding> {
        compare(&parse(reference).unwrap().root, &parse(candidate).unwrap().root)
    }

    fn kinds(f: &[Finding]) -> Vec<Kind> {
        f.iter().map(|x| x.kind).collect()
    }

    #[test]
    fn identical_documents_have_no_findings() {
        let x = "<a xmlns:n='urn:n'><n:b>1</n:b><c><d>2026-01-01</d></c></a>";
        assert!(run(x, x).is_empty());
        // A different prefix for the same namespace is not a difference.
        let y = "<a xmlns:m='urn:n'><m:b>5</m:b><c><d>2027-12-31</d></c></a>";
        assert!(run(x, y).is_empty());
    }

    #[test]
    fn detects_namespace_and_rename() {
        let f = run(
            "<a xmlns:n='urn:n'><n:partner/><modellrechnung><x/></modellrechnung></a>",
            "<a><partner/><modellberechnung><x/></modellberechnung></a>",
        );
        assert_eq!(kinds(&f), vec![Kind::Namespace, Kind::Misnamed]);
        assert!(f[1].title.contains("`modellberechnung` should probably be `modellrechnung`"));
    }

    #[test]
    fn detects_missing_wrapper() {
        let f = run(
            "<a><list><entry><v>1</v></entry><entry><v>2</v></entry></list></a>",
            "<a><entry><v>1</v></entry><entry><w>2</w></entry></a>",
        );
        assert_eq!(f[0].kind, Kind::Misplaced);
        assert!(f[0].title.contains("should be wrapped in `list`"));
        // The wrapped content is still compared.
        assert!(f.iter().any(|x| x.kind == Kind::Unexpected && x.title.contains("`w`")));
    }

    #[test]
    fn groups_missing_and_order() {
        let f = run("<a><b/><c/><d/><e/></a>", "<a><e/><b/></a>");
        assert_eq!(kinds(&f), vec![Kind::Missing, Kind::Order]);
        assert_eq!(f[0].items.len(), 2);
        assert_eq!(f[1].items.len(), 1);
    }

    #[test]
    fn detects_repetition_and_incomplete_entries() {
        let f = run(
            "<a><z>1</z><r><x>1</x><y>1</y></r><r><x>1</x><y>1</y></r></a>",
            "<a><z>1</z><z>2</z><r><x>1</x><y>1</y></r><r><x>1</x></r></a>",
        );
        assert_eq!(kinds(&f), vec![Kind::Repetition, Kind::Incomplete]);
    }

    #[test]
    fn explains_renames() {
        assert_eq!(describe_rename("flexPhase", "flexphase"), "Only upper and lower case differ.");
        assert!(describe_rename("PlusEins", "PlusEinJahr").starts_with("“s” should be “Jahr”"));
        assert!(describe_rename("fonds", "fond").starts_with("“s” is extra"));
    }
}
