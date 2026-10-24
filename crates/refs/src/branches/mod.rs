//! The branch picker and its dialogs: switch, create, merge, rebase, delete, rename, set the
//! upstream, pin; remote branches and the ones another worktree holds.

mod dialogs;
mod picker;

pub use dialogs::{open_new_branch, open_rename_branch, open_set_upstream};
pub use picker::{BranchPicker, branch_chip};

use crate::names::{Branch, sanitized_ref_name};

pub const PINNED: &str = "Pinned";
pub const RECENT: &str = "Recent";
pub const LOCAL: &str = "Local";
/// How many recent branches are listed.
pub const RECENT_MAX: usize = 5;

/// Pinned, recently checked out, the rest of local, then one group per remote (origin first),
/// each by recency. A branch shows in one group only; the current one first among local.
pub fn groups(
    branches: &[Branch],
    query: &str,
    pins: &[String],
    visited: &[String],
) -> Vec<(String, Vec<Branch>)> {
    let q = query.trim().to_lowercase();
    // "fix login" finds fix-login, which is what it would be created as.
    let typed = sanitized_ref_name(&q);
    let matches = |b: &Branch| {
        let n = b.name.to_lowercase();
        n.contains(&q) || (!typed.is_empty() && n.contains(&typed))
    };
    let local = |n: &str| branches.iter().find(|b| !b.remote && b.name == n);
    let pinned: Vec<&String> = pins
        .iter()
        .filter(|n| branches.iter().any(|b| &b.name == *n))
        .collect();
    let recent: Vec<&String> = visited
        .iter()
        .filter(|n| local(n).is_some_and(|b| !b.current) && !pinned.contains(n))
        .take(RECENT_MAX)
        .collect();
    let mut found: Vec<&Branch> = branches.iter().filter(|b| matches(b)).collect();
    found.sort_by_key(|b| !b.current);
    let mut remotes: Vec<&str> = found.iter().filter_map(|b| b.remote_name()).collect();
    remotes.sort_by(|a, b| (*b == "origin").cmp(&(*a == "origin")).then(a.cmp(b)));
    remotes.dedup();
    let mut out: Vec<(String, Vec<Branch>)> = vec![
        (PINNED.into(), vec![]),
        (RECENT.into(), vec![]),
        (LOCAL.into(), vec![]),
    ];
    out.extend(remotes.iter().map(|r| (r.to_string(), vec![])));
    for b in found {
        let group = if pinned.contains(&&b.name) {
            PINNED.to_string()
        } else if recent.contains(&&b.name) {
            RECENT.to_string()
        } else if let Some(r) = b.remote_name() {
            r.to_string()
        } else {
            LOCAL.to_string()
        };
        if let Some((_, list)) = out.iter_mut().find(|(g, _)| *g == group) {
            list.push(b.clone());
        }
    }
    // Pins in the order they were pinned, recent ones newest first.
    let order = |names: &[&String], list: &mut Vec<Branch>| {
        list.sort_by_key(|b| names.iter().position(|n| **n == b.name))
    };
    order(&pinned, &mut out[0].1);
    order(&recent, &mut out[1].1);
    out.retain(|(_, l)| !l.is_empty());
    out
}

/// What's typed, as git would create it, when no branch has that name: then the picker offers to.
pub fn create_name(branches: &[Branch], query: &str) -> Option<String> {
    let typed = query.trim();
    let name = sanitized_ref_name(typed);
    let exact = branches.iter().any(|b| {
        b.name == typed
            || b.name == name
            || b.short_name() == name
            || (!b.remote && crate::names::same_ref(&b.name, &name, crate::names::FOLD_CASE))
    });
    (!name.is_empty() && !exact).then_some(name)
}

/// The worktree a branch is checked out in, other than this one. Switching to origin/x means
/// switching to x, so a remote row goes with its local branch.
pub fn held_in(b: &Branch, branches: &[Branch]) -> Option<String> {
    if b.current {
        return None;
    }
    if b.remote {
        let local = b.short_name();
        return branches
            .iter()
            .find(|x| !x.remote && !x.current && x.name == local)
            .and_then(|x| x.worktree.clone());
    }
    b.worktree.clone()
}

/// Whether a branch's row offers Delete: not the one checked out or held by a worktree, not a
/// remote's default, and not where GitHub would refuse it anyway: a protected branch (`guarded`,
/// as "origin/main"), or a remote the account can't push to (`push`: by remote name).
pub fn deletable(
    b: &Branch,
    held: bool,
    guarded: &std::collections::HashSet<String>,
    push: impl Fn(&str) -> Option<bool>,
) -> bool {
    let refused = b.remote && b.remote_name().is_some_and(|r| push(r) == Some(false));
    !b.current && !held && !b.remote_default && !guarded.contains(&b.name) && !refused
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_is_offered_only_where_it_can_work() {
        let guarded: std::collections::HashSet<String> = ["origin/main".to_string()].into();
        let push = |r: &str| match r {
            "origin" => Some(true),
            "up" => Some(false),
            _ => None,
        };
        assert!(deletable(&b("feat"), false, &guarded, push));
        assert!(deletable(&b("origin/feat"), false, &guarded, push));
        // Protected on GitHub; its local twin isn't.
        assert!(!deletable(&b("origin/main"), false, &guarded, push));
        assert!(deletable(&b("main"), false, &guarded, push));
        // The original's branches, where the account can't push.
        assert!(!deletable(&b("up/x"), false, &guarded, push));
        // Checked out, held by another worktree, or the remote's default.
        let mut cur = b("feat");
        cur.current = true;
        assert!(!deletable(&cur, false, &guarded, push));
        assert!(!deletable(&b("feat"), true, &guarded, push));
        let mut head = b("origin/dev");
        head.remote_default = true;
        assert!(!deletable(&head, false, &guarded, push));
    }

    fn b(name: &str) -> Branch {
        Branch {
            name: name.into(),
            remote: name.starts_with("origin/") || name.starts_with("up/"),
            ..Default::default()
        }
    }

    #[test]
    fn groups_put_each_branch_once_pins_and_recent_first() {
        let mut list = vec![
            b("main"),
            b("feat"),
            b("fix-login"),
            b("old"),
            b("origin/main"),
            b("up/x"),
        ];
        list[0].current = true;
        let names = |g: &[(String, Vec<Branch>)]| {
            g.iter()
                .map(|(n, l)| {
                    (
                        n.clone(),
                        l.iter().map(|b| b.name.clone()).collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let g = groups(&list, "", &["old".into()], &["main".into(), "feat".into()]);
        assert_eq!(
            names(&g),
            vec![
                ("Pinned".into(), vec!["old".to_string()]),
                ("Recent".into(), vec!["feat".to_string()]),
                (
                    "Local".into(),
                    vec!["main".to_string(), "fix-login".to_string()]
                ),
                ("origin".into(), vec!["origin/main".to_string()]),
                ("up".into(), vec!["up/x".to_string()]),
            ]
        );
        // "fix login" finds fix-login.
        assert_eq!(
            names(&groups(&list, "fix login", &[], &[])),
            vec![("Local".into(), vec!["fix-login".to_string()])]
        );
    }

    #[test]
    fn what_is_typed_is_offered_only_when_new() {
        let list = vec![b("main"), b("origin/feature")];
        assert_eq!(
            create_name(&list, "fix login").as_deref(),
            Some("fix-login")
        );
        assert_eq!(create_name(&list, "main"), None);
        assert_eq!(create_name(&list, "feature"), None);
        assert_eq!(create_name(&list, "  "), None);
    }

    #[test]
    fn a_remote_row_is_held_where_its_local_branch_is() {
        let mut feat = b("feat");
        feat.worktree = Some("/w/feat".into());
        let list = vec![feat.clone(), b("origin/feat")];
        assert_eq!(held_in(&list[1], &list).as_deref(), Some("/w/feat"));
        assert_eq!(held_in(&feat, &list).as_deref(), Some("/w/feat"));
        assert_eq!(held_in(&b("main"), &list), None);
    }
}
