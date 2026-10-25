//! A worktree's viewed marks: `kind:path` → the file's content signature when it was marked, in
//! the order they were set.

use std::collections::{BTreeMap, HashMap};

/// Marks on a commit's or a range's files never lapse on a change, so only the latest are kept;
/// the working tree's always are.
const MAX_FIXED: usize = 2000;

fn fixed(key: &str) -> bool {
    key.starts_with("commit:") || key.starts_with("pr-file:")
}

#[derive(Debug, Default)]
pub struct ViewedMarks {
    /// Key → (when it was set, the signature).
    marks: HashMap<String, (u64, String)>,
    /// When each was set → its key: the order, oldest first.
    order: BTreeMap<u64, String>,
    next: u64,
    fixed: usize,
    max_fixed: usize,
}

impl ViewedMarks {
    pub fn new() -> Self {
        Self::with_limit(MAX_FIXED)
    }

    fn with_limit(max_fixed: usize) -> Self {
        Self {
            max_fixed,
            ..Self::default()
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.marks.get(key).map(|(_, sig)| sig.as_str())
    }

    pub fn len(&self) -> usize {
        self.marks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// The keys, the oldest set first.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.order.values()
    }

    /// Key and signature, the oldest set first.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.order
            .values()
            .map(|k| (k.as_str(), self.marks[k].1.as_str()))
    }

    /// Marks `key` at `sig` as the latest; past the limit the oldest commit and range marks go.
    pub fn set(&mut self, key: String, sig: String) {
        self.remove(&key);
        let at = self.next;
        self.next += 1;
        if fixed(&key) {
            self.fixed += 1;
        }
        self.order.insert(at, key.clone());
        self.marks.insert(key, (at, sig));
        if self.fixed > self.max_fixed {
            self.prune();
        }
    }

    pub fn remove(&mut self, key: &str) -> bool {
        let Some((at, _)) = self.marks.remove(key) else {
            return false;
        };
        self.order.remove(&at);
        if fixed(key) {
            self.fixed -= 1;
        }
        true
    }

    pub fn clear(&mut self) {
        self.marks.clear();
        self.order.clear();
        self.fixed = 0;
    }

    fn prune(&mut self) {
        let over = self.fixed - self.max_fixed;
        let oldest: Vec<String> = self
            .order
            .values()
            .filter(|k| fixed(k))
            .take(over)
            .cloned()
            .collect();
        for key in oldest {
            self.remove(&key);
        }
    }
}

impl FromIterator<(String, String)> for ViewedMarks {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(marks: I) -> Self {
        let mut out = Self::new();
        out.extend(marks);
        out
    }
}

impl Extend<(String, String)> for ViewedMarks {
    fn extend<I: IntoIterator<Item = (String, String)>>(&mut self, marks: I) {
        for (key, sig) in marks {
            self.set(key, sig);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(m: &ViewedMarks) -> Vec<&str> {
        m.keys().map(String::as_str).collect()
    }

    #[test]
    fn only_the_oldest_commit_and_range_marks_go_never_a_working_tree_one() {
        let marks = [
            ("unstaged:a.ts", "M:1"),
            ("commit:aaa:a.ts", "M:1"),
            ("pr-file:5@b..c:a.ts", "M:2"),
            ("branch:base:a.ts", "M:3"),
            ("commit:bbb:a.ts", "A:4"),
        ];
        let with = |max: usize| {
            let mut m = ViewedMarks::with_limit(max);
            m.extend(marks.iter().map(|(k, v)| (k.to_string(), v.to_string())));
            m
        };
        assert_eq!(
            keys(&with(2)),
            [
                "unstaged:a.ts",
                "pr-file:5@b..c:a.ts",
                "branch:base:a.ts",
                "commit:bbb:a.ts"
            ]
        );
        // Under the limit: all stay.
        assert_eq!(with(10).len(), 5);
        assert_eq!(keys(&with(0)), ["unstaged:a.ts", "branch:base:a.ts"]);
    }

    #[test]
    fn a_mark_set_again_is_the_latest_and_keeps_one_place() {
        let mut m = ViewedMarks::with_limit(2);
        for k in ["commit:a:x", "commit:b:x", "unstaged:y"] {
            m.set(k.into(), "1".into());
        }
        m.set("commit:a:x".into(), "2".into());
        assert_eq!(keys(&m), ["commit:b:x", "unstaged:y", "commit:a:x"]);
        assert_eq!(m.get("commit:a:x"), Some("2"));
        // The one set again outlives the one it was older than.
        m.set("commit:c:x".into(), "1".into());
        assert_eq!(keys(&m), ["unstaged:y", "commit:a:x", "commit:c:x"]);
        assert!(m.remove("commit:a:x"));
        assert!(!m.remove("commit:a:x"));
        assert_eq!(
            m.iter().collect::<Vec<_>>(),
            [("unstaged:y", "1"), ("commit:c:x", "1")]
        );
    }

    #[test]
    fn a_long_review_plateaus_at_the_limit() {
        let mut m = ViewedMarks::new();
        for i in 0..MAX_FIXED + 500 {
            m.set(format!("commit:{i}:a.rs"), "M:1".into());
            m.set(format!("pr-file:{i}@a..b:a.rs"), "M:1".into());
        }
        m.set("unstaged:a.rs".into(), "M:1".into());
        assert_eq!(m.len(), MAX_FIXED + 1);
        assert_eq!(m.order.len(), m.len());
        assert!(m.get("unstaged:a.rs").is_some());
        assert!(m.get("commit:0:a.rs").is_none());
        let last = MAX_FIXED + 499;
        assert!(m.get(&format!("pr-file:{last}@a..b:a.rs")).is_some());
    }
}
