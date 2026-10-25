//! History's edits of the branch, as data: which commits an edit names, what it keeps, the
//! branch's own first-parent line, and where dragged commits land. Pure.

use std::collections::HashSet;

/// A rewrite of the branch's history, as core's `rewrite::Edit` takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    Reword {
        sha: String,
        message: String,
    },
    /// `shas` and `onto` become one commit where `onto` is; with a message it's the commit's
    /// (squash), without, the oldest one's stays (fixup).
    Squash {
        shas: Vec<String>,
        onto: String,
        message: Option<String>,
    },
    Drop {
        shas: Vec<String>,
    },
    Move {
        sha: String,
        up: bool,
    },
    Reorder {
        shas: Vec<String>,
        before: Option<String>,
    },
    Split {
        sha: String,
    },
    FixupStaged {
        sha: String,
    },
}

impl Edit {
    pub fn to_core(&self) -> gitviber_core::rewrite::Edit {
        use gitviber_core::rewrite::Edit as E;
        match self.clone() {
            Edit::Reword { sha, message } => E::Reword { sha, message },
            Edit::Squash {
                shas,
                onto,
                message,
            } => E::Squash {
                shas,
                onto,
                message,
            },
            Edit::Drop { shas } => E::Drop { shas },
            Edit::Move { sha, up } => E::Move { sha, up },
            Edit::Reorder { shas, before } => E::Reorder { shas, before },
            Edit::Split { sha } => E::Split { sha },
            Edit::FixupStaged { sha } => E::FixupStaged { sha },
        }
    }

    pub fn verb(&self) -> &'static str {
        match self {
            Edit::Reword { .. } => "Reword",
            Edit::Squash { message: None, .. } | Edit::FixupStaged { .. } => "Fixup",
            Edit::Squash { .. } => "Squash",
            Edit::Drop { .. } => "Drop",
            Edit::Move { .. } | Edit::Reorder { .. } => "Move",
            Edit::Split { .. } => "Split",
        }
    }
}

/// A commit as the edits need it: its id and parents.
pub trait Node {
    fn sha(&self) -> &str;
    fn parents(&self) -> &[String];
}

/// The commits an edit names; it makes everything from the oldest of them on again. `about`:
/// the commits it's about.
pub fn edited_shas<N: Node>(edit: &Edit, about: &[N]) -> Vec<String> {
    match edit {
        Edit::Reword { sha, .. } | Edit::Split { sha } | Edit::FixupStaged { sha } => {
            vec![sha.clone()]
        }
        // Down swaps it with its parent.
        Edit::Move { sha, up } => {
            let mut v = vec![sha.clone()];
            if !up {
                v.extend(about.first().and_then(|c| c.parents().first().cloned()));
            }
            v
        }
        Edit::Squash { shas, onto, .. } => shas.iter().cloned().chain([onto.clone()]).collect(),
        Edit::Reorder { shas, before } => shas.iter().cloned().chain(before.clone()).collect(),
        Edit::Drop { shas } => shas.clone(),
    }
}

/// HEAD's first-parent line among commits listed newest first, which history edits can name:
/// merged-in side branches' commits aren't on it. Pages come in one after another, so the walk
/// goes on where the last page ran out rather than from HEAD again: a parent always comes after
/// its children in the list (topo order).
#[derive(Default)]
pub struct FirstParentLine {
    /// By index in the list.
    on: Vec<bool>,
    /// The next commit on the line, not listed yet.
    next: Option<String>,
}

impl FirstParentLine {
    pub fn new(head: Option<&str>) -> Self {
        Self {
            on: vec![],
            next: head.map(str::to_string),
        }
    }

    /// Walks on into `commits[from..]`, the page just listed.
    pub fn extend<N: Node>(&mut self, commits: &[N], from: usize) {
        self.on.resize(commits.len(), false);
        let Some(mut next) = self.next.take() else {
            return;
        };
        let at: std::collections::HashMap<&str, usize> = commits
            .iter()
            .enumerate()
            .skip(from)
            .map(|(i, c)| (c.sha(), i))
            .collect();
        while let Some(&i) = at.get(next.as_str()) {
            if std::mem::replace(&mut self.on[i], true) {
                return;
            }
            let Some(parent) = commits[i].parents().first() else {
                return;
            };
            next = parent.clone();
        }
        self.next = Some(next);
    }

    pub fn contains(&self, i: usize) -> bool {
        self.on.get(i).copied().unwrap_or(false)
    }
}

/// What the rewrite keeps as it is, for asking whether it drops pushed commits: the parent of the
/// oldest of `shas` in `commits` (newest first). None from the root: nothing is kept, so any
/// pushed commit counts. One not listed (a search's gap, past the loaded page) has no parent
/// known here: None too, the cautious answer, as it is made again itself.
pub fn kept_up_to<N: Node>(shas: &[String], commits: &[N]) -> Option<String> {
    let at: Vec<Option<usize>> = shas
        .iter()
        .map(|s| commits.iter().position(|c| c.sha() == s))
        .collect();
    if at.iter().any(Option::is_none) {
        return None;
    }
    let oldest = at.into_iter().flatten().max()?;
    commits[oldest].parents().first().cloned()
}

/// Where dragged commits would land: on a commit (squash) or above or below it (move).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Where {
    Above,
    Onto,
    Below,
}

/// Where commits dropped in a gap go, for a reorder edit: just under `before`, the nearest commit
/// above the gap that isn't moving, or on top (Some(None)). None when they're there already.
/// `line`: the listed commits, newest first.
pub fn reorder_before(
    line: &[String],
    moved: &HashSet<String>,
    sha: &str,
    at: Where,
) -> Option<Option<String>> {
    let i = line.iter().position(|s| s == sha).unwrap_or(0);
    let before = if at == Where::Below {
        Some(sha.to_string())
    } else {
        line[..i]
            .iter()
            .rev()
            .find(|s| !moved.contains(*s))
            .cloned()
    };
    let rest: Vec<&String> = line.iter().filter(|s| !moved.contains(*s)).collect();
    let k = match &before {
        None => 0,
        Some(b) => rest.iter().position(|s| *s == b).map_or(0, |p| p + 1),
    };
    let mut next: Vec<&String> = rest[..k].to_vec();
    next.extend(line.iter().filter(|s| moved.contains(*s)));
    next.extend(&rest[k..]);
    // Only spares the toast: core checks the move against the branch's first-parent line.
    if next.iter().zip(line).all(|(a, b)| *a == b) {
        None
    } else {
        Some(before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct C(String, Vec<String>);
    impl Node for C {
        fn sha(&self) -> &str {
            &self.0
        }
        fn parents(&self) -> &[String] {
            &self.1
        }
    }

    fn line() -> Vec<String> {
        ["e", "d", "c", "b", "a"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn place(moved: &[&str], sha: &str, at: Where) -> Option<Option<String>> {
        let moved: HashSet<String> = moved.iter().map(|s| s.to_string()).collect();
        reorder_before(&line(), &moved, sha, at)
    }

    fn some(s: &str) -> Option<Option<String>> {
        Some(Some(s.to_string()))
    }

    #[test]
    fn a_gap_names_the_commit_the_moved_ones_go_under() {
        assert_eq!(place(&["a"], "c", Where::Below), some("c"));
        assert_eq!(place(&["a"], "c", Where::Above), some("d"));
        assert_eq!(place(&["a"], "e", Where::Above), Some(None));
        assert_eq!(place(&["d", "a"], "c", Where::Above), some("e"));
        assert_eq!(place(&["e", "a"], "d", Where::Above), Some(None));
    }

    #[test]
    fn dropping_where_they_are_changes_nothing() {
        assert_eq!(place(&["c"], "d", Where::Below), None);
        assert_eq!(place(&["c"], "b", Where::Above), None);
        assert_eq!(place(&["e"], "d", Where::Above), None);
        assert_eq!(place(&["c", "b"], "a", Where::Above), None);
        assert_eq!(place(&["d", "b"], "c", Where::Above), some("e"));
    }

    #[test]
    fn the_ends_of_the_list() {
        assert_eq!(place(&["c"], "a", Where::Below), some("a"));
        assert_eq!(place(&["e", "d", "c", "b"], "a", Where::Above), None);
        assert_eq!(place(&["e", "d"], "a", Where::Below), some("a"));
    }

    #[test]
    fn a_gap_between_picked_commits_gathers_them() {
        assert_eq!(place(&["d", "b"], "c", Where::Below), some("c"));
        assert_eq!(place(&["e", "a"], "c", Where::Above), some("d"));
    }

    fn listed() -> Vec<C> {
        let shas = ["e", "d", "c", "b", "a"];
        shas.iter()
            .enumerate()
            .map(|(i, s)| {
                C(
                    s.to_string(),
                    shas.get(i + 1)
                        .map(|p| vec![p.to_string()])
                        .unwrap_or_default(),
                )
            })
            .collect()
    }

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn an_edit_names_the_commits_it_starts_from() {
        let l = listed();
        let none: &[C] = &[];
        assert_eq!(
            edited_shas(
                &Edit::Reword {
                    sha: "c".into(),
                    message: "m".into()
                },
                &l[2..3]
            ),
            v(&["c"])
        );
        assert_eq!(
            edited_shas(
                &Edit::Move {
                    sha: "d".into(),
                    up: true
                },
                &l[1..2]
            ),
            v(&["d"])
        );
        assert_eq!(
            edited_shas(
                &Edit::Move {
                    sha: "d".into(),
                    up: false
                },
                &l[1..2]
            ),
            v(&["d", "c"])
        );
        assert_eq!(
            edited_shas(
                &Edit::Squash {
                    shas: v(&["e"]),
                    onto: "b".into(),
                    message: None
                },
                &l[..1]
            ),
            v(&["e", "b"])
        );
        assert_eq!(
            edited_shas(
                &Edit::Reorder {
                    shas: v(&["e"]),
                    before: Some("c".into())
                },
                &l[..1]
            ),
            v(&["e", "c"])
        );
        assert_eq!(
            edited_shas(
                &Edit::Reorder {
                    shas: v(&["c"]),
                    before: None
                },
                none
            ),
            v(&["c"])
        );
        assert_eq!(
            edited_shas(
                &Edit::Drop {
                    shas: v(&["d", "b"])
                },
                none
            ),
            v(&["d", "b"])
        );
        assert_eq!(
            edited_shas(&Edit::Split { sha: "c".into() }, none),
            v(&["c"])
        );
        assert_eq!(
            edited_shas(&Edit::FixupStaged { sha: "b".into() }, none),
            v(&["b"])
        );
    }

    #[test]
    fn the_rewrite_keeps_whats_under_its_oldest_commit() {
        let l = listed();
        assert_eq!(kept_up_to(&v(&["e", "c"]), &l).as_deref(), Some("b"));
        assert_eq!(kept_up_to(&v(&["d", "a"]), &l), None);
        let all = edited_shas(
            &Edit::Squash {
                shas: v(&["e", "d", "c", "b"]),
                onto: "a".into(),
                message: None,
            },
            &l,
        );
        assert_eq!(kept_up_to(&all, &l), None);
        let one = edited_shas(
            &Edit::Squash {
                shas: v(&["c"]),
                onto: "a".into(),
                message: Some("m".into()),
            },
            &l[2..3],
        );
        assert_eq!(kept_up_to(&one, &l), None);
        // Not listed yet (the next page, a search's gap): rewritten too, so anything pushed counts.
        assert_eq!(kept_up_to(&v(&["e", "z"]), &l), None);
        assert_eq!(kept_up_to(&v(&["z"]), &l), None);
    }

    #[test]
    fn the_first_parent_line_leaves_a_merged_in_side_branch_out() {
        let side = vec![
            C("m".into(), v(&["c", "s"])),
            C("s".into(), v(&["b"])),
            C("c".into(), v(&["b"])),
            C("b".into(), v(&["a"])),
            C("a".into(), vec![]),
        ];
        let on = |l: &FirstParentLine| {
            (0..side.len())
                .filter(|i| l.contains(*i))
                .collect::<Vec<_>>()
        };
        let mut whole = FirstParentLine::new(Some("m"));
        whole.extend(&side, 0);
        assert_eq!(on(&whole), [0, 2, 3, 4]);
        // Page by page, the walk goes on where it ran out.
        let mut paged = FirstParentLine::new(Some("m"));
        paged.extend(&side[..3], 0);
        assert_eq!(on(&paged), [0, 2]);
        paged.extend(&side, 3);
        assert_eq!(on(&paged), [0, 2, 3, 4]);
        let mut gone = FirstParentLine::new(Some("x"));
        gone.extend(&side, 0);
        assert!(on(&gone).is_empty());
    }
}
