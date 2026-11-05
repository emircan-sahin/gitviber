//! What a PR or issue page may do and how its buttons read.

use crate::model::{Access, Account, IssueItem, PullItem, repo_of};
use crate::requests::PullSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    pub const ALL: [MergeMethod; 3] =
        [MergeMethod::Merge, MergeMethod::Squash, MergeMethod::Rebase];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Merge => "Create a merge commit",
            Self::Squash => "Squash and merge",
            Self::Rebase => "Rebase and merge",
        }
    }

    /// The Merge button's own word.
    pub fn button(self) -> &'static str {
        match self {
            Self::Merge => "Merge",
            other => other.label(),
        }
    }
}

/// What the repository lets a PR merge with, in GitHub's order; the first is the Merge button's
/// (as gh pr merge picks). GitHub tells writers only: unknown counts as allowed.
pub fn allowed_methods(access: Option<&Access>) -> Vec<MergeMethod> {
    let allowed = |m: MergeMethod| match m {
        MergeMethod::Merge => access.and_then(|a| a.allow_merge_commit),
        MergeMethod::Squash => access.and_then(|a| a.allow_squash_merge),
        MergeMethod::Rebase => access.and_then(|a| a.allow_rebase_merge),
    };
    let methods: Vec<MergeMethod> = MergeMethod::ALL
        .into_iter()
        .filter(|m| allowed(*m) != Some(false))
        .collect();
    if methods.is_empty() {
        MergeMethod::ALL.to_vec()
    } else {
        methods
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewEvent {
    Comment,
    Approve,
    RequestChanges,
}

impl ReviewEvent {
    pub const ALL: [ReviewEvent; 3] = [
        ReviewEvent::Comment,
        ReviewEvent::Approve,
        ReviewEvent::RequestChanges,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Comment => "COMMENT",
            Self::Approve => "APPROVE",
            Self::RequestChanges => "REQUEST_CHANGES",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Comment => "Comment",
            Self::Approve => "Approve",
            Self::RequestChanges => "Request changes",
        }
    }

    pub fn note(self) -> &'static str {
        match self {
            Self::Comment => "General feedback without explicit approval.",
            Self::Approve => "Give your approval to merge these changes.",
            Self::RequestChanges => "Feedback that must be addressed before merging.",
        }
    }

    pub fn done(self) -> &'static str {
        match self {
            Self::Comment => "Review submitted",
            Self::Approve => "Approved",
            Self::RequestChanges => "Changes requested",
        }
    }

    /// GitHub wants a note unless approving.
    pub fn needs_body(self) -> bool {
        self != Self::Approve
    }
}

/// A verdict's words in a PR's conversation.
pub fn review_badge(state: &str) -> String {
    match state {
        "APPROVED" => "approved".into(),
        "CHANGES_REQUESTED" => "requested changes".into(),
        "COMMENTED" => "reviewed".into(),
        "DISMISSED" => "dismissed".into(),
        other => other.to_lowercase(),
    }
}

/// Where Checkout puts a PR: its own branch when that lives on origin (a pushed fix then updates
/// the PR), otherwise pr/<n>, or pr/<owner>/<n> for a fork's original, which numbers its own.
/// Unknown until the account loads: a guess could fetch another repo's same-named branch.
pub fn pull_source(p: &PullItem, account: Option<&Account>) -> Option<PullSource> {
    let account = account?;
    let origin = account.origin.as_ref().map(Access::full_name);
    let same_repo = origin.as_ref().is_some_and(|o| {
        p.head_repo
            .as_ref()
            .is_some_and(|h| h.eq_ignore_ascii_case(o))
    });
    let in_origin = account
        .access_for(&p.url)
        .is_some_and(|a| account.is_origin(a));
    let target = repo_of(&p.url);
    let branch = if same_repo {
        p.head_ref.clone()
    } else if in_origin {
        format!("pr/{}", p.number)
    } else {
        format!("pr/{}/{}", target.split('/').next().unwrap_or(""), p.number)
    };
    Some(PullSource {
        target,
        number: p.number,
        head_ref: p.head_ref.clone(),
        same_repo,
        branch,
    })
}

/// Who may do what on a PR: GitHub lets its author and triagers close and reopen it, but an
/// author can't reopen what a maintainer closed; merging needs write access.
pub struct PullRights {
    pub merge: bool,
    pub close: bool,
    pub reopen: bool,
    /// The PR's branch is on origin, where a pushed fix updates it.
    pub resolve: bool,
    pub own: bool,
}

pub fn pull_rights(p: &PullItem, account: Option<&Account>, closed_by: Option<&str>) -> PullRights {
    let access = account.and_then(|a| a.access_for(&p.url));
    let login = account.map(|a| a.login.as_str());
    let own = login == Some(p.author.as_str());
    let writer = access.is_some_and(|a| a.push);
    let triager = access.is_some_and(|a| a.triage);
    let origin = account
        .and_then(|a| a.origin.as_ref())
        .map(Access::full_name);
    let same_repo = origin.is_some_and(|o| {
        p.head_repo
            .as_ref()
            .is_some_and(|h| h.eq_ignore_ascii_case(&o))
    });
    PullRights {
        merge: writer,
        close: writer || triager || own,
        reopen: writer || triager || (own && closed_by.is_some() && closed_by == login),
        resolve: same_repo && access.is_some(),
        own,
    }
}

/// Who may do what on an issue: its author edits, closes and reopens it without write access,
/// triagers close and reopen anyone's; labels aren't the author's to set; admins alone delete.
pub struct IssueRights {
    pub edit: bool,
    pub close: bool,
    pub reopen: bool,
    pub label: bool,
    pub delete: bool,
}

pub fn issue_rights(
    i: &IssueItem,
    account: Option<&Account>,
    closed_by: Option<&str>,
) -> IssueRights {
    let access = account.and_then(|a| a.access_for(&i.url));
    let login = account.map(|a| a.login.as_str());
    let own = login == Some(i.author.as_str());
    let writer = access.is_some_and(|a| a.push);
    let triager = access.is_some_and(|a| a.triage);
    IssueRights {
        edit: writer || own,
        close: writer || own || triager,
        reopen: writer || triager || (own && closed_by.is_some() && closed_by == login),
        label: writer || triager,
        delete: access.is_some_and(|a| a.admin),
    }
}

/// GitHub's title for a PR of several commits: the branch name, spaced and capitalized.
pub fn branch_title(branch: &str) -> String {
    let spaced: String = branch
        .chars()
        .map(|c| if c == '-' || c == '_' { ' ' } else { c })
        .collect();
    let spaced = spaced
        .split(' ')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut chars = spaced.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PullState;

    fn access(full: &str, push: bool) -> Access {
        let (owner, name) = full.split_once('/').unwrap();
        Access {
            owner: owner.into(),
            name: name.into(),
            default_branch: Some("main".into()),
            push,
            triage: false,
            admin: false,
            issues: true,
            allow_merge_commit: None,
            allow_squash_merge: None,
            allow_rebase_merge: None,
        }
    }

    fn account(origin: &str, parent: Option<&str>) -> Account {
        Account {
            login: "me".into(),
            source: "gh".into(),
            origin: Some(access(origin, true)),
            parent: parent.map(|p| access(p, false)),
        }
    }

    fn pull(url_repo: &str, head_repo: &str, author: &str) -> PullItem {
        PullItem {
            number: 7,
            title: String::new(),
            state: PullState::Open,
            draft: false,
            author: author.into(),
            head_ref: "fix".into(),
            head_sha: "abc".into(),
            head_repo: Some(head_repo.into()),
            base_ref: "main".into(),
            base_sha: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            url: format!("https://github.com/{url_repo}/pull/7"),
        }
    }

    #[test]
    fn checkout_names_the_branch_by_where_it_lives() {
        let me = account("me/app", Some("up/app"));
        assert_eq!(
            pull_source(&pull("me/app", "Me/App", "me"), Some(&me))
                .unwrap()
                .branch,
            "fix"
        );
        assert_eq!(
            pull_source(&pull("me/app", "other/app", "x"), Some(&me))
                .unwrap()
                .branch,
            "pr/7"
        );
        let up = pull_source(&pull("up/app", "other/app", "x"), Some(&me)).unwrap();
        assert_eq!(
            (up.branch.as_str(), up.target.as_str(), up.same_repo),
            ("pr/up/7", "up/app", false)
        );
        assert!(pull_source(&pull("me/app", "me/app", "me"), None).is_none());
    }

    #[test]
    fn merge_methods_follow_the_repository_or_all_when_unknown() {
        let mut a = access("o/r", true);
        assert_eq!(allowed_methods(Some(&a)), MergeMethod::ALL);
        a.allow_merge_commit = Some(false);
        assert_eq!(
            allowed_methods(Some(&a)),
            [MergeMethod::Squash, MergeMethod::Rebase]
        );
        a.allow_squash_merge = Some(false);
        a.allow_rebase_merge = Some(false);
        assert_eq!(allowed_methods(Some(&a)), MergeMethod::ALL);
    }

    #[test]
    fn an_author_reopens_only_what_they_closed() {
        let mut me = account("o/r", None);
        me.origin.as_mut().unwrap().push = false;
        let p = pull("o/r", "o/r", "me");
        assert!(pull_rights(&p, Some(&me), None).close);
        assert!(!pull_rights(&p, Some(&me), Some("maintainer")).reopen);
        assert!(pull_rights(&p, Some(&me), Some("me")).reopen);
        assert!(!pull_rights(&p, Some(&me), None).merge);
    }

    #[test]
    fn a_branch_name_titles_a_pr_of_several_commits() {
        assert_eq!(branch_title("fix-login_bug"), "Fix login bug");
        assert_eq!(branch_title(""), "");
    }
}
