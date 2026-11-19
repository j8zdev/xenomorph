//! Tracks findings across re-checks: what is new, still open or fixed
//! compared with a baseline, and what the user dismissed.

use crate::compare::{Finding, Kind};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Open,
    New,
    Fixed,
}

#[derive(Clone, Debug)]
pub struct Tracked {
    pub finding: Finding,
    pub status: Status,
    /// What changed inside a still-open finding since the baseline.
    pub change: String,
}

#[derive(Default)]
pub struct Session {
    baseline: Option<Vec<Finding>>,
    pub tracked: Vec<Tracked>,
    dismissed_keys: HashSet<String>,
    dismissed_kinds: HashSet<Kind>,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Counts {
    pub open: usize,
    pub new: usize,
    pub fixed: usize,
    pub dismissed: usize,
}

impl Session {
    /// Forgets everything except dismissals. The next update becomes the baseline.
    pub fn restart(&mut self) {
        self.baseline = None;
        self.tracked.clear();
    }

    /// Makes the current findings the baseline: nothing is new or fixed any more.
    pub fn accept_baseline(&mut self) {
        let current: Vec<Finding> =
            self.tracked.iter().filter(|t| t.status != Status::Fixed).map(|t| t.finding.clone()).collect();
        self.baseline = None;
        self.update(current);
    }

    pub fn update(&mut self, findings: Vec<Finding>) {
        let baseline = self.baseline.get_or_insert_with(|| findings.clone());
        let mut tracked: Vec<Tracked> = Vec::new();
        for f in &findings {
            match baseline.iter().find(|b| b.key == f.key) {
                None => tracked.push(Tracked { finding: f.clone(), status: Status::New, change: String::new() }),
                Some(old) => {
                    let ids = |x: &Finding| x.items.iter().map(|i| i.id.clone()).collect::<HashSet<_>>();
                    let (before, now) = (ids(old), ids(f));
                    let resolved = before.difference(&now).count();
                    let added = now.difference(&before).count();
                    let mut parts = Vec::new();
                    if resolved > 0 {
                        parts.push(format!("{resolved} of {} resolved", before.len()));
                    }
                    if added > 0 {
                        parts.push(format!("{added} added"));
                    }
                    tracked.push(Tracked { finding: f.clone(), status: Status::Open, change: parts.join(", ") });
                }
            }
        }
        for old in baseline.iter() {
            if !findings.iter().any(|f| f.key == old.key) {
                tracked.push(Tracked { finding: old.clone(), status: Status::Fixed, change: String::new() });
            }
        }
        tracked.sort_by_key(|t| t.finding.kind);
        self.tracked = tracked;
    }

    pub fn is_dismissed(&self, t: &Tracked) -> bool {
        self.dismissed_keys.contains(&t.finding.key) || self.dismissed_kinds.contains(&t.finding.kind)
    }

    pub fn kind_dismissed(&self, kind: Kind) -> bool {
        self.dismissed_kinds.contains(&kind)
    }

    pub fn toggle_finding(&mut self, key: &str) {
        if !self.dismissed_keys.remove(key) {
            self.dismissed_keys.insert(key.to_string());
        }
    }

    /// Dismisses or restores every finding of a kind, including ones that
    /// show up later.
    pub fn toggle_kind(&mut self, kind: Kind) {
        if !self.dismissed_kinds.remove(&kind) {
            self.dismissed_kinds.insert(kind);
        }
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for t in &self.tracked {
            if t.status == Status::Fixed {
                c.fixed += 1;
            } else if self.is_dismissed(t) {
                c.dismissed += 1;
            } else if t.status == Status::New {
                c.new += 1;
            } else {
                c.open += 1;
            }
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::compare;
    use crate::shape::parse;

    fn findings(reference: &str, candidate: &str) -> Vec<Finding> {
        compare(&parse(reference).unwrap().root, &parse(candidate).unwrap().root)
    }

    #[test]
    fn tracks_new_open_and_fixed() {
        let reference = "<a><b/><c/><d/><e><x/></e></a>";
        let mut s = Session::default();
        s.update(findings(reference, "<a><b/><e/></a>"));
        // Baseline: c and d missing in a, e is empty.
        assert_eq!(s.counts(), Counts { open: 2, new: 0, fixed: 0, dismissed: 0 });

        // d added, e filled, an unexpected element introduced.
        s.update(findings(reference, "<a><b/><d/><e><x/></e><zzz/></a>"));
        assert_eq!(s.counts(), Counts { open: 1, new: 1, fixed: 1, dismissed: 0 });
        let open = s.tracked.iter().find(|t| t.status == Status::Open).unwrap();
        assert_eq!(open.change, "1 of 2 resolved");

        s.toggle_kind(Kind::Unexpected);
        assert_eq!(s.counts(), Counts { open: 1, new: 0, fixed: 1, dismissed: 1 });

        s.accept_baseline();
        assert_eq!(s.counts(), Counts { open: 1, new: 0, fixed: 0, dismissed: 1 });
    }
}
