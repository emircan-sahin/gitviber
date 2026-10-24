//! A commit as the lists keep it, and its decorations grouped so they stay readable.

use std::rc::Rc;

use gitviber_core::git;
use gpui_kit::SharedString;

use crate::edits::Node;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub short: SharedString,
    pub author: SharedString,
    pub email: SharedString,
    /// When it was written (author date).
    pub timestamp: i64,
    pub committer: SharedString,
    /// When it landed on the branch: the list's order.
    pub committed_at: i64,
    pub parents: Vec<String>,
    /// As git decorates: "HEAD -> main", "origin/main", "tag: v1".
    pub refs: Vec<String>,
    pub subject: SharedString,
    pub body: SharedString,
    pub unpushed: bool,
    pub on_origin: bool,
    pub not_in_head: bool,
    /// A followed file's history: its path in this commit.
    pub file: Option<String>,
}

impl From<git::Commit> for Commit {
    fn from(c: git::Commit) -> Self {
        Commit {
            sha: c.sha,
            short: c.short_sha.into(),
            author: c.author_name.into(),
            email: c.author_email.into(),
            timestamp: c.timestamp,
            committer: c.committer_name.into(),
            committed_at: c.committed_at,
            parents: c.parents,
            refs: c.refs,
            subject: c.subject.into(),
            body: c.body.into(),
            unpushed: c.unpushed,
            on_origin: c.on_origin,
            not_in_head: c.not_in_head,
            file: c.file,
        }
    }
}

impl Commit {
    /// From what a saved tab or another feature holds.
    pub fn from_core(c: &git::Commit) -> Self {
        Commit {
            sha: c.sha.clone(),
            short: c.short_sha.clone().into(),
            author: c.author_name.clone().into(),
            email: c.author_email.clone().into(),
            timestamp: c.timestamp,
            committer: c.committer_name.clone().into(),
            committed_at: c.committed_at,
            parents: c.parents.clone(),
            refs: c.refs.clone(),
            subject: c.subject.clone().into(),
            body: c.body.clone().into(),
            unpushed: c.unpushed,
            on_origin: c.on_origin,
            not_in_head: c.not_in_head,
            file: c.file.clone(),
        }
    }

    /// As a selection holds it.
    pub fn to_core(&self) -> git::Commit {
        git::Commit {
            sha: self.sha.clone(),
            short_sha: self.short.to_string(),
            author_name: self.author.to_string(),
            author_email: self.email.to_string(),
            timestamp: self.timestamp,
            committer_name: self.committer.to_string(),
            committed_at: self.committed_at,
            parents: self.parents.clone(),
            refs: self.refs.clone(),
            subject: self.subject.to_string(),
            body: self.body.to_string(),
            unpushed: self.unpushed,
            on_origin: self.on_origin,
            not_in_head: self.not_in_head,
            file: self.file.clone(),
        }
    }

    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }

    /// Subject and body, as Copy message and a reword start from.
    pub fn message(&self) -> String {
        let body = self.body.trim();
        if body.is_empty() {
            self.subject.to_string()
        } else {
            format!("{}\n\n{body}", self.subject)
        }
    }

    pub fn tags(&self) -> Vec<&str> {
        self.refs
            .iter()
            .filter_map(|r| r.strip_prefix("tag: "))
            .collect()
    }
}

impl Node for Commit {
    fn sha(&self) -> &str {
        &self.sha
    }
    fn parents(&self) -> &[String] {
        &self.parents
    }
}

impl Node for Rc<Commit> {
    fn sha(&self) -> &str {
        &self.sha
    }
    fn parents(&self) -> &[String] {
        &self.parents
    }
}

/// Which refs the all-branches graph walks, kept per repository.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GraphRefs {
    pub local: bool,
    pub remote: bool,
    pub tags: bool,
    /// Full ref names left out.
    pub hidden: Vec<String>,
    /// Just this ref's history.
    pub only: Option<String>,
}

impl Default for GraphRefs {
    fn default() -> Self {
        GraphRefs {
            local: true,
            remote: true,
            tags: true,
            hidden: vec![],
            only: None,
        }
    }
}

impl GraphRefs {
    pub fn to_core(&self) -> git::GraphRefs {
        git::GraphRefs {
            local: self.local,
            remote: self.remote,
            tags: self.tags,
            hidden: self.hidden.clone(),
            only: self.only.clone(),
        }
    }

    /// These hidden too. Hiding the solo'd branch shows everything else.
    pub fn hide(&self, refs: &[String]) -> GraphRefs {
        let mut hidden = self.hidden.clone();
        for r in refs {
            if !hidden.contains(r) {
                hidden.push(r.clone());
            }
        }
        GraphRefs {
            only: None,
            hidden,
            ..self.clone()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Head,
    Local,
    Remote,
    Tag,
}

/// A badge: `refs` are the full names it stands for (a local branch and its remote twin).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Badge {
    pub name: String,
    pub kind: RefKind,
    /// A local branch with its remote twin on the same commit.
    pub synced: bool,
    pub refs: Vec<String>,
}

/// Groups decorations: a local branch and its remote twin on the same commit (main + origin/main)
/// become one "main ☁" badge; origin/HEAD is dropped, a detached HEAD gets a badge of its own.
/// `remotes` tells remote-tracking names apart, since local names can contain "/" too. With
/// `show`, only the refs it lets through, and always HEAD's branch.
pub fn group_refs(refs: &[String], remotes: &[String], show: Option<&GraphRefs>) -> Vec<Badge> {
    let head = refs.iter().find_map(|r| r.strip_prefix("HEAD -> "));
    let is_remote = |r: &str| {
        remotes.iter().any(|x| x == r) || (remotes.is_empty() && r.starts_with("origin/"))
    };
    let short = |r: &str| r.split_once('/').map_or(r, |(_, b)| b).to_string();
    let tag = |r: &str| r.strip_prefix("tag: ").map(str::to_string);
    let full = |r: &str| match tag(r) {
        Some(t) => format!("refs/tags/{t}"),
        None if is_remote(r) => format!("refs/remotes/{r}"),
        None => format!("refs/heads/{r}"),
    };
    // Before grouping: with local branches off, main's remote twin still gets its badge.
    let shown = |r: &str| match show {
        None => true,
        Some(s) => {
            r == "HEAD"
                || Some(r) == head
                || ((if tag(r).is_some() {
                    s.tags
                } else if is_remote(r) {
                    s.remote
                } else {
                    s.local
                }) && !s.hidden.contains(&full(r)))
        }
    };
    let names: Vec<&str> = refs
        .iter()
        .map(|r| r.strip_prefix("HEAD -> ").unwrap_or(r))
        .filter(|r| shown(r))
        .collect();
    let mut out = vec![];
    if names.contains(&"HEAD") {
        out.push(Badge {
            name: "HEAD".into(),
            kind: RefKind::Head,
            synced: false,
            refs: vec![],
        });
    }
    for r in &names {
        if *r == "HEAD" || r.ends_with("/HEAD") {
            continue;
        }
        if let Some(t) = tag(r) {
            out.push(Badge {
                name: t,
                kind: RefKind::Tag,
                synced: false,
                refs: vec![full(r)],
            });
        } else if is_remote(r) {
            if !names.contains(&short(r).as_str()) {
                out.push(Badge {
                    name: r.to_string(),
                    kind: RefKind::Remote,
                    synced: false,
                    refs: vec![full(r)],
                });
            }
        } else {
            let twins: Vec<&&str> = names
                .iter()
                .filter(|x| is_remote(x) && short(x) == *r)
                .collect();
            let mut all = vec![full(r)];
            all.extend(twins.iter().map(|t| full(t)));
            out.push(Badge {
                name: r.to_string(),
                kind: if Some(*r) == head {
                    RefKind::Head
                } else {
                    RefKind::Local
                },
                synced: !twins.is_empty(),
                refs: all,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_branch_and_its_remote_twin_are_one_badge() {
        let b = group_refs(
            &s(&[
                "HEAD -> main",
                "origin/main",
                "origin/HEAD",
                "tag: v1",
                "feat",
            ]),
            &s(&["origin/main", "origin/HEAD"]),
            None,
        );
        assert_eq!(
            b.iter()
                .map(|b| (b.name.as_str(), b.kind, b.synced))
                .collect::<Vec<_>>(),
            vec![
                ("main", RefKind::Head, true),
                ("v1", RefKind::Tag, false),
                ("feat", RefKind::Local, false)
            ]
        );
        assert_eq!(
            b[0].refs,
            s(&["refs/heads/main", "refs/remotes/origin/main"])
        );
    }

    #[test]
    fn a_detached_head_and_a_lone_remote_branch_get_their_own() {
        let b = group_refs(&s(&["HEAD", "origin/fix"]), &s(&["origin/fix"]), None);
        assert_eq!(
            b.iter()
                .map(|b| (b.name.as_str(), b.kind))
                .collect::<Vec<_>>(),
            vec![("HEAD", RefKind::Head), ("origin/fix", RefKind::Remote)]
        );
    }

    #[test]
    fn the_graphs_choices_hide_badges_but_never_heads_branch() {
        let show = GraphRefs {
            local: false,
            tags: false,
            ..Default::default()
        };
        let b = group_refs(
            &s(&["HEAD -> main", "origin/main", "feat", "tag: v1"]),
            &s(&["origin/main"]),
            Some(&show),
        );
        assert_eq!(
            b.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(),
            vec!["main"]
        );
        let hidden = GraphRefs {
            hidden: s(&["refs/heads/feat"]),
            ..Default::default()
        };
        let b = group_refs(
            &s(&["feat", "origin/feat"]),
            &s(&["origin/feat"]),
            Some(&hidden),
        );
        assert_eq!(
            b.iter()
                .map(|b| (b.name.as_str(), b.kind))
                .collect::<Vec<_>>(),
            vec![("origin/feat", RefKind::Remote)]
        );
    }
}
