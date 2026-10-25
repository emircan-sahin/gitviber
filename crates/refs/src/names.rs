//! Branch and tag names: what git takes, what a typed one becomes, what a new worktree checks
//! out, and how a branch stands with its upstream. Pure, as the pickers and dialogs share it.

use gitviber_core::git;

/// git's empty tree (SHA-1), what changes before a repository's first commit are against.
pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Refs are files, often, and macOS and Windows file systems ignore case: there Master is master.
pub const FOLD_CASE: bool = cfg!(any(target_os = "macos", windows));

/// A local or remote-tracking branch as the UI keeps it: core's `git::Branch`, cloneable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub remote: bool,
    pub current: bool,
    pub upstream: Option<String>,
    pub sha: String,
    pub timestamp: i64,
    /// Checked out in another worktree (its path).
    pub worktree: Option<String>,
    pub merged: bool,
    pub remote_default: bool,
}

impl From<git::Branch> for Branch {
    fn from(b: git::Branch) -> Self {
        Branch {
            name: b.name,
            remote: b.remote,
            current: b.current,
            upstream: b.upstream,
            sha: b.sha,
            timestamp: b.timestamp,
            worktree: b.worktree,
            merged: b.merged,
            remote_default: b.remote_default,
        }
    }
}

impl Branch {
    /// As the graph's refs name it: refs/heads/… or refs/remotes/….
    pub fn full_ref(&self) -> String {
        format!(
            "{}{}",
            if self.remote {
                "refs/remotes/"
            } else {
                "refs/heads/"
            },
            self.name
        )
    }

    /// A remote's name ends at the first "/", as everywhere in the app.
    pub fn remote_name(&self) -> Option<&str> {
        self.remote.then(|| {
            self.name
                .split_once('/')
                .map_or(self.name.as_str(), |(r, _)| r)
        })
    }

    /// origin/feat → feat; a local branch's own name.
    pub fn short_name(&self) -> &str {
        if self.remote {
            self.name
                .split_once('/')
                .map_or(self.name.as_str(), |(_, b)| b)
        } else {
            &self.name
        }
    }
}

/// refs/heads/main → main, refs/remotes/origin/main → origin/main.
pub fn short_ref(r: &str) -> &str {
    ["refs/heads/", "refs/remotes/", "refs/tags/"]
        .iter()
        .find_map(|p| r.strip_prefix(p))
        .unwrap_or(r)
}

/// The last part of a path: a worktree's folder name.
pub fn folder_name(path: &str) -> &str {
    let p = path.trim_end_matches('/');
    p.rsplit('/').next().unwrap_or(p)
}

/// "1 commit", "3 commits".
pub fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn invalid(c: char) -> bool {
    (c as u32) <= 0x20 || c == '\x7f' || "~^:?*[\\|\"<>".contains(c)
}

/// A typed branch or tag name as git will take it: "fix login bug" → "fix-login-bug". What git
/// refuses becomes "-": control characters and space, DEL, ~ ^ : ? * [ \ | " < >, "@{", "..", a
/// component starting with a dot, a trailing dot or "/", a component ending in ".lock"; then
/// "//" is one, a leading - + . or / goes, and HEAD gets a dash.
pub fn sanitized_ref_name(name: &str) -> String {
    let c: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len());
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if invalid(ch) {
            while i < c.len() && invalid(c[i]) {
                i += 1;
            }
            out.push('-');
            continue;
        }
        if ch == '@' && c.get(i + 1) == Some(&'{') {
            out.push('-');
            i += 2;
            continue;
        }
        if ch == '.' {
            if c.get(i + 1) == Some(&'.') {
                while i < c.len() && c[i] == '.' {
                    i += 1;
                }
                out.push('-');
                continue;
            }
            let lock = c[i + 1..].starts_with(&['l', 'o', 'c', 'k'])
                && c.get(i + 5).is_none_or(|n| *n == '/');
            if i == 0 || c[i - 1] == '/' || i + 1 == c.len() || lock {
                out.push('-');
                i += if lock && !(i == 0 || c[i - 1] == '/') {
                    5
                } else {
                    1
                };
                continue;
            }
        }
        if ch == '/' && i + 1 == c.len() {
            out.push('-');
            i += 1;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    let mut collapsed = String::with_capacity(out.len());
    for ch in out.chars() {
        if ch == '/' && collapsed.ends_with('/') {
            continue;
        }
        collapsed.push(ch);
    }
    let s = collapsed.trim_start_matches(['-', '+', '.', '/']);
    if s == "HEAD" || s == "@" {
        format!("{s}-")
    } else {
        s.to_string()
    }
}

/// Whether two branch names are the same branch here.
pub fn same_ref(a: &str, b: &str, fold: bool) -> bool {
    if fold {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

/// Local branch names but `except`: what a new branch name must not be.
pub fn local_names(branches: &[Branch], except: Option<&str>) -> Vec<String> {
    branches
        .iter()
        .filter(|b| !b.remote && Some(b.name.as_str()) != except)
        .map(|b| b.name.clone())
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameCheck {
    pub name: String,
    pub taken: bool,
    /// What it became, or that a branch has it.
    pub hint: Option<String>,
}

/// A typed name as it will be used, and the line to show under it.
pub fn ref_name_check(typed: &str, existing: &[String], rename: bool, fold: bool) -> NameCheck {
    let typed = typed.trim();
    let name = sanitized_ref_name(typed);
    let clash = (!name.is_empty())
        .then(|| existing.iter().find(|n| same_ref(n, &name, fold)))
        .flatten();
    let hint = match clash {
        Some(c) => Some(format!("A branch named {c} already exists.")),
        None if !name.is_empty() && name != typed => Some(format!(
            "Will be {} {name}.",
            if rename { "renamed to" } else { "created as" }
        )),
        None => None,
    };
    NameCheck {
        taken: clash.is_some(),
        name,
        hint,
    }
}

/// What a branch review compares with at first: origin's default branch as last fetched, else a
/// local branch of that name; None when there's neither.
pub fn review_base(branches: &[Branch]) -> Option<String> {
    let name = branches
        .iter()
        .find(|b| b.remote_default && b.name.starts_with("origin/"))
        .map_or("main", |b| &b.name["origin/".len()..]);
    if branches
        .iter()
        .any(|b| b.remote && b.name == format!("origin/{name}"))
    {
        return Some(format!("refs/remotes/origin/{name}"));
    }
    branches
        .iter()
        .any(|b| !b.remote && b.name == name)
        .then(|| format!("refs/heads/{name}"))
}

/// What add_worktree gets as its base: the one picked, none (an existing branch as it is), or a ref.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorktreeBase {
    Picked,
    Existing,
    Ref(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeBranch {
    pub name: String,
    pub base: WorktreeBase,
    /// The new branch tracks `base`.
    pub track: bool,
    pub hint: Option<String>,
    pub taken: bool,
}

/// What a new worktree for the typed name checks out. A local branch as it is, unless a worktree
/// has it already (git keeps a branch in one at a time). A branch only on a remote, as a local one
/// tracking it: origin's where several remotes have it, or the one typed as `upstream/feat`. Else a
/// new branch, from the base picked.
pub fn worktree_branch(typed: &str, branches: &[Branch], fold: bool) -> WorktreeBranch {
    let check = ref_name_check(typed, &local_names(branches, None), false, fold);
    let remotes: Vec<&Branch> = branches.iter().filter(|b| b.remote).collect();
    let local_of = |n: &str| {
        branches
            .iter()
            .find(|b| !b.remote && same_ref(&b.name, n, fold))
    };
    // "upstream/feat" is feat from upstream: a local branch by that name would be ambiguous.
    let named = if local_of(&check.name).is_some() {
        None
    } else {
        remotes.iter().find(|b| b.name == check.name).copied()
    };
    let name = named.map_or(check.name.clone(), |b| b.short_name().to_string());
    if let Some(local) = local_of(&name) {
        let held = if local.current {
            Some("here".to_string())
        } else {
            local
                .worktree
                .as_deref()
                .map(|w| format!("in {}", folder_name(w)))
        };
        return match held {
            Some(held) => WorktreeBranch {
                name: local.name.clone(),
                base: WorktreeBase::Picked,
                track: false,
                hint: Some(format!(
                    "{} is checked out {held}; a branch can be in one worktree at a time.",
                    local.name
                )),
                taken: true,
            },
            None => WorktreeBranch {
                name: local.name.clone(),
                base: WorktreeBase::Existing,
                track: false,
                hint: Some(format!("Checks out the existing branch {}.", local.name)),
                taken: false,
            },
        };
    }
    let namesakes: Vec<&Branch> = remotes
        .iter()
        .filter(|b| b.short_name() == name)
        .copied()
        .collect();
    let remote = named
        .or_else(|| {
            namesakes
                .iter()
                .find(|b| b.name.starts_with("origin/"))
                .copied()
        })
        .or_else(|| namesakes.first().copied());
    if let Some(remote) = remote {
        return WorktreeBranch {
            base: WorktreeBase::Ref(format!("refs/remotes/{}", remote.name)),
            track: true,
            hint: Some(format!(
                "Checks out {} as a new tracking branch {name}.",
                remote.name
            )),
            taken: false,
            name,
        };
    }
    let prefix = remotes
        .iter()
        .map(|b| format!("{}/", b.remote_name().unwrap_or_default()))
        .find(|p| name.starts_with(p.as_str()));
    if let Some(prefix) = prefix {
        return WorktreeBranch {
            hint: Some(format!(
                "{prefix} is a remote's; a branch named {name} would be ambiguous."
            )),
            name,
            base: WorktreeBase::Picked,
            track: false,
            taken: true,
        };
    }
    WorktreeBranch {
        name,
        base: WorktreeBase::Picked,
        track: false,
        hint: check.hint,
        taken: false,
    }
}

/// Where a new worktree starts: the branch checked out here. Detached, the default branch, as
/// core's default_branch finds it: local if there is one.
pub fn worktree_base(branches: &[Branch]) -> String {
    if let Some(current) = branches.iter().find(|b| b.current && !b.remote) {
        return format!("refs/heads/{}", current.name);
    }
    let remote = branches
        .iter()
        .find(|b| b.remote_default && b.name.starts_with("origin/"));
    let name = remote.map_or("main", |r| &r.name["origin/".len()..]);
    if branches.iter().any(|b| !b.remote && b.name == name) {
        return format!("refs/heads/{name}");
    }
    remote.map_or_else(
        || "HEAD".to_string(),
        |r| format!("refs/remotes/{}", r.name),
    )
}

/// How a local branch stands with its upstream: a few characters to show, and words for the tooltip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Standing {
    pub text: String,
    pub label: String,
}

/// `t`: (ahead, behind, gone), when known. None when even, or remote.
pub fn branch_tracking(
    remote: bool,
    upstream: Option<&str>,
    t: Option<(u32, u32, bool)>,
) -> Option<Standing> {
    if remote {
        return None;
    }
    let (ahead, behind, gone) = t.unwrap_or((0, 0, false));
    if gone {
        let label = match upstream {
            Some(u) => format!("Its upstream {u} is gone from the remote"),
            None => "Its upstream is gone from the remote".to_string(),
        };
        return Some(Standing {
            text: "upstream gone".into(),
            label,
        });
    }
    let Some(upstream) = upstream else {
        return Some(Standing {
            text: "local only".into(),
            label: "Not published: it has no upstream".into(),
        });
    };
    let mut parts = vec![];
    if ahead > 0 {
        parts.push((
            format!("↑{ahead}"),
            format!("{} ahead", plural(ahead as usize, "commit")),
        ));
    }
    if behind > 0 {
        parts.push((
            format!("↓{behind}"),
            format!("{} behind", plural(behind as usize, "commit")),
        ));
    }
    if parts.is_empty() {
        return None;
    }
    Some(Standing {
        text: parts
            .iter()
            .map(|p| p.0.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        label: format!(
            "Compared with {upstream}: {}",
            parts
                .iter()
                .map(|p| p.1.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    })
}

/// "feature-a", "feature-a and feature-b", "feature-a, feature-b and 2 more".
pub fn move_along(branches: &[String]) -> String {
    let shown = &branches[..branches.len().min(3)];
    let rest = branches.len() - shown.len();
    if rest > 0 {
        return format!("{} and {rest} more", shown.join(", "));
    }
    match shown {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(name: &str, remote: bool, remote_default: bool) -> Branch {
        Branch {
            name: name.into(),
            remote,
            remote_default,
            ..Default::default()
        }
    }

    #[test]
    fn a_review_starts_from_origins_default_branch_else_the_local_one() {
        assert_eq!(
            review_base(&[
                b("feature", false, false),
                b("main", false, false),
                b("origin/main", true, false)
            ])
            .as_deref(),
            Some("refs/remotes/origin/main")
        );
        assert_eq!(
            review_base(&[
                b("dev", false, false),
                b("origin/dev", true, true),
                b("origin/main", true, false)
            ])
            .as_deref(),
            Some("refs/remotes/origin/dev")
        );
        assert_eq!(
            review_base(&[b("feature", false, false), b("main", false, false)]).as_deref(),
            Some("refs/heads/main")
        );
        assert_eq!(
            review_base(&[b("feature", false, false), b("master", false, false)]),
            None
        );
    }

    #[test]
    fn a_typed_ref_name_becomes_one_git_takes() {
        let cases = [
            ("fix login bug", "fix-login-bug"),
            ("feat/ok-name_1.2", "feat/ok-name_1.2"),
            ("a~b^c:d?e*f[g\\h|i\"j<k>l", "a-b-c-d-e-f-g-h-i-j-k-l"),
            ("a  \t b", "a-b"),
            ("x@{1}..y", "x-1}-y"),
            (".hidden.", "hidden-"),
            ("topic.lock", "topic-"),
            ("topic/", "topic-"),
            ("--+-force", "force"),
            ("ünïcode-ok", "ünïcode-ok"),
            ("-.hidden", "hidden"),
            ("~.x", "x"),
            ("-/a", "a"),
            ("/a", "a"),
            ("a/.b", "a/-b"),
            ("a/.git", "a/-git"),
            ("a/./b", "a/-/b"),
            ("a.lock/b", "a-/b"),
            ("a/b.lock/c", "a/b-/c"),
            ("a//b", "a/b"),
            ("HEAD", "HEAD-"),
            ("@", "@-"),
        ];
        for (typed, name) in cases {
            assert_eq!(sanitized_ref_name(typed), name, "{typed}");
        }
    }

    #[test]
    fn a_typed_name_says_what_it_becomes_or_that_its_taken() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            ref_name_check(" fix login ", &s(&["main"]), false, false),
            NameCheck {
                name: "fix-login".into(),
                taken: false,
                hint: Some("Will be created as fix-login.".into())
            }
        );
        assert_eq!(
            ref_name_check("fix login", &s(&["fix-login"]), false, false),
            NameCheck {
                name: "fix-login".into(),
                taken: true,
                hint: Some("A branch named fix-login already exists.".into())
            }
        );
        assert_eq!(
            ref_name_check("feat", &s(&["main"]), false, false),
            NameCheck {
                name: "feat".into(),
                taken: false,
                hint: None
            }
        );
        assert_eq!(
            ref_name_check("  ", &[], false, false),
            NameCheck {
                name: "".into(),
                taken: false,
                hint: None
            }
        );
        assert_eq!(
            ref_name_check("a b", &[], true, false).hint.as_deref(),
            Some("Will be renamed to a-b.")
        );
        assert_eq!(
            ref_name_check("Master", &s(&["master"]), false, true)
                .hint
                .as_deref(),
            Some("A branch named master already exists.")
        );
        assert!(!ref_name_check("Master", &s(&["master"]), false, false).taken);
    }

    fn row(
        name: &str,
        remote: bool,
        current: bool,
        worktree: Option<&str>,
        remote_default: bool,
    ) -> Branch {
        Branch {
            name: name.into(),
            remote,
            current,
            worktree: worktree.map(Into::into),
            remote_default,
            ..Default::default()
        }
    }

    #[test]
    fn a_new_worktree_checks_out_what_the_typed_name_already_is() {
        let branches = vec![
            row("main", false, true, None, false),
            row("feat", false, false, None, false),
            row("held", false, false, Some("/w/repo.worktrees/held"), false),
            row("origin/main", true, false, None, false),
            row("origin/remote-only", true, false, None, false),
            row("upstream/remote-only", true, false, None, false),
            row("upstream/theirs", true, false, None, false),
        ];
        let pick = |typed: &str| {
            let w = worktree_branch(typed, &branches, false);
            (w.name, w.base, w.track, w.taken)
        };
        use WorktreeBase::*;
        assert_eq!(
            pick("new thing"),
            ("new-thing".into(), Picked, false, false)
        );
        assert_eq!(pick("feat"), ("feat".into(), Existing, false, false));
        assert_eq!(pick("main"), ("main".into(), Picked, false, true));
        assert_eq!(pick("held"), ("held".into(), Picked, false, true));
        assert!(
            worktree_branch("held", &branches, false)
                .hint
                .unwrap()
                .contains("checked out in held;")
        );
        assert_eq!(
            pick("remote-only"),
            (
                "remote-only".into(),
                Ref("refs/remotes/origin/remote-only".into()),
                true,
                false
            )
        );
        assert_eq!(
            pick("upstream/remote-only"),
            (
                "remote-only".into(),
                Ref("refs/remotes/upstream/remote-only".into()),
                true,
                false
            )
        );
        assert_eq!(
            pick("theirs"),
            (
                "theirs".into(),
                Ref("refs/remotes/upstream/theirs".into()),
                true,
                false
            )
        );
        assert_eq!(pick("origin/main"), ("main".into(), Picked, false, true));
        assert_eq!(
            pick("origin/nope"),
            ("origin/nope".into(), Picked, false, true)
        );
    }

    #[test]
    fn a_new_worktree_starts_from_the_current_branch_else_the_default_one() {
        assert_eq!(
            worktree_base(&[
                row("main", false, false, None, false),
                row("feat", false, true, None, false)
            ]),
            "refs/heads/feat"
        );
        assert_eq!(
            worktree_base(&[
                row("dev", false, false, None, false),
                row("origin/dev", true, false, None, true)
            ]),
            "refs/heads/dev"
        );
        assert_eq!(
            worktree_base(&[row("origin/dev", true, false, None, true)]),
            "refs/remotes/origin/dev"
        );
        assert_eq!(
            worktree_base(&[row("main", false, false, None, false)]),
            "refs/heads/main"
        );
        assert_eq!(worktree_base(&[]), "HEAD");
    }

    #[test]
    fn a_local_branchs_standing_with_its_upstream() {
        let t = |a: u32, b: u32, g: bool| Some((a, b, g));
        let up = Some("origin/feat");
        assert_eq!(branch_tracking(false, up, None), None);
        assert_eq!(
            branch_tracking(false, up, t(2, 0, false)),
            Some(Standing {
                text: "↑2".into(),
                label: "Compared with origin/feat: 2 commits ahead".into()
            })
        );
        assert_eq!(
            branch_tracking(false, up, t(1, 3, false)),
            Some(Standing {
                text: "↑1 ↓3".into(),
                label: "Compared with origin/feat: 1 commit ahead, 3 commits behind".into()
            })
        );
        assert_eq!(
            branch_tracking(false, up, t(0, 1, false)),
            Some(Standing {
                text: "↓1".into(),
                label: "Compared with origin/feat: 1 commit behind".into()
            })
        );
        assert_eq!(
            branch_tracking(false, None, None),
            Some(Standing {
                text: "local only".into(),
                label: "Not published: it has no upstream".into()
            })
        );
        assert_eq!(
            branch_tracking(false, up, t(0, 0, true)),
            Some(Standing {
                text: "upstream gone".into(),
                label: "Its upstream origin/feat is gone from the remote".into()
            })
        );
        assert_eq!(branch_tracking(true, None, None), None);
    }

    #[test]
    fn move_along_names_the_branches() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(move_along(&s(&["a"])), "a");
        assert_eq!(move_along(&s(&["a", "b"])), "a and b");
        assert_eq!(move_along(&s(&["a", "b", "c"])), "a, b and c");
        assert_eq!(
            move_along(&s(&["a", "b", "c", "d", "e"])),
            "a, b, c and 2 more"
        );
    }

    #[test]
    fn a_count_takes_the_plural_unless_its_one() {
        assert_eq!(plural(1, "commit"), "1 commit");
        assert_eq!(plural(0, "commit"), "0 commits");
        assert_eq!(plural(3, "worktree"), "3 worktrees");
        assert_eq!(plural(2, "uncommitted change"), "2 uncommitted changes");
    }

    #[test]
    fn short_refs_and_folders() {
        assert_eq!(short_ref("refs/remotes/origin/main"), "origin/main");
        assert_eq!(short_ref("HEAD"), "HEAD");
        assert_eq!(folder_name("/w/repo.worktrees/held/"), "held");
    }
}
