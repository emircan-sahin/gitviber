//! GitHub's answers as the views keep them: small clonable copies of core's replies. A list keeps
//! its rows, a tab keys itself by its item; bodies and threads stay with the tab that read them.

use gitviber_core::github;

/// Where a request goes: None is origin, else owner/name of origin or the repository it was
/// forked from (core refuses anything else). A PR's or issue's own url says which.
pub type Target = Option<String>;

/// CI's verdict on a commit, from GitHub's status rollup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CiState {
    Success,
    Failure,
    Pending,
}

impl CiState {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "success" => Some(Self::Success),
            "failure" => Some(Self::Failure),
            "pending" => Some(Self::Pending),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "Checks passed",
            Self::Failure => "Checks failed",
            Self::Pending => "Checks running",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PullState {
    Open,
    Closed,
    Merged,
}

impl PullState {
    fn parse(s: &str) -> Self {
        match s {
            "merged" => Self::Merged,
            "closed" => Self::Closed,
            _ => Self::Open,
        }
    }

    /// Open beats merged beats closed, for a branch's badge.
    pub fn rank(self) -> u8 {
        match self {
            Self::Open => 3,
            Self::Merged => 2,
            Self::Closed => 1,
        }
    }
}

/// A pull request as a list shows it, and what its tab is opened with.
#[derive(Clone, Debug, PartialEq)]
pub struct PullItem {
    pub number: u64,
    pub title: String,
    pub state: PullState,
    pub draft: bool,
    pub author: String,
    pub head_ref: String,
    pub head_sha: String,
    /// owner/name of the repository the branch lives in; None once that fork is gone.
    pub head_repo: Option<String>,
    pub base_ref: String,
    pub base_sha: String,
    pub created_at: String,
    pub updated_at: String,
    pub url: String,
}

impl PullItem {
    /// As core has it, for what takes core's pull request (a guided review of it).
    pub fn to_core(&self) -> github::Pull {
        github::Pull {
            number: self.number,
            title: self.title.clone(),
            state: match self.state {
                PullState::Open => "open",
                PullState::Closed => "closed",
                PullState::Merged => "merged",
            }
            .into(),
            draft: self.draft,
            author: self.author.clone(),
            head_ref: self.head_ref.clone(),
            head_sha: self.head_sha.clone(),
            head_repo: self.head_repo.clone(),
            base_ref: self.base_ref.clone(),
            base_sha: self.base_sha.clone(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            url: self.url.clone(),
        }
    }
}

impl From<&github::Pull> for PullItem {
    fn from(p: &github::Pull) -> Self {
        PullItem {
            number: p.number,
            title: p.title.clone(),
            state: PullState::parse(&p.state),
            draft: p.draft,
            author: p.author.clone(),
            head_ref: p.head_ref.clone(),
            head_sha: p.head_sha.clone(),
            head_repo: p.head_repo.clone(),
            base_ref: p.base_ref.clone(),
            base_sha: p.base_sha.clone(),
            created_at: p.created_at.clone(),
            updated_at: p.updated_at.clone(),
            url: p.url.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LabelItem {
    pub name: String,
    /// Hex without the '#', as its author typed it.
    pub color: String,
    pub description: String,
}

impl From<&github::Label> for LabelItem {
    fn from(l: &github::Label) -> Self {
        LabelItem {
            name: l.name.clone(),
            color: l.color.clone(),
            description: l.description.clone(),
        }
    }
}

impl LabelItem {
    /// Only a plain six-digit hex reaches the screen.
    pub fn rgb(&self) -> Option<u32> {
        (self.color.len() == 6 && self.color.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| u32::from_str_radix(&self.color, 16).ok())
            .flatten()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct IssueItem {
    pub number: u64,
    pub title: String,
    pub open: bool,
    /// "completed" | "not_planned" | "reopened", when GitHub recorded one.
    pub state_reason: Option<String>,
    pub author: String,
    pub labels: Vec<LabelItem>,
    pub assignees: Vec<String>,
    /// Comment count.
    pub comments: u64,
    pub created_at: String,
    pub updated_at: String,
    pub url: String,
}

impl From<&github::Issue> for IssueItem {
    fn from(i: &github::Issue) -> Self {
        IssueItem {
            number: i.number,
            title: i.title.clone(),
            open: i.state == "open",
            state_reason: i.state_reason.clone(),
            author: i.author.clone(),
            labels: i.labels.iter().map(LabelItem::from).collect(),
            assignees: i.assignees.clone(),
            comments: i.comments,
            created_at: i.created_at.clone(),
            updated_at: i.updated_at.clone(),
            url: i.url.clone(),
        }
    }
}

impl IssueItem {
    pub fn not_planned(&self) -> bool {
        !self.open && self.state_reason.as_deref() == Some("not_planned")
    }
}

/// What the account may do in one repository.
#[derive(Clone, Debug, PartialEq)]
pub struct Access {
    pub owner: String,
    pub name: String,
    pub default_branch: Option<String>,
    /// Write access: merge, and close or edit anyone's PRs and issues.
    pub push: bool,
    /// Close and reopen anyone's PRs and issues, without write access.
    pub triage: bool,
    /// The only role GitHub lets delete issues.
    pub admin: bool,
    /// Issues are switched on (forks start with them off).
    pub issues: bool,
    pub allow_merge_commit: Option<bool>,
    pub allow_squash_merge: Option<bool>,
    pub allow_rebase_merge: Option<bool>,
}

impl Access {
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

impl From<&github::Access> for Access {
    fn from(a: &github::Access) -> Self {
        Access {
            owner: a.repo.owner.clone(),
            name: a.repo.name.clone(),
            default_branch: a.default_branch.clone(),
            push: a.push,
            triage: a.triage,
            admin: a.admin,
            issues: a.issues,
            allow_merge_commit: a.allow_merge_commit,
            allow_squash_merge: a.allow_squash_merge,
            allow_rebase_merge: a.allow_rebase_merge,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub login: String,
    /// "gh" (GitHub CLI) or "git" (git's credential store).
    pub source: String,
    pub origin: Option<Access>,
    /// The repository origin was forked from.
    pub parent: Option<Access>,
}

impl From<&github::Account> for Account {
    fn from(a: &github::Account) -> Self {
        Account {
            login: a.login.clone(),
            source: a.source.clone(),
            origin: a.origin.as_ref().map(Access::from),
            parent: a.parent.as_ref().map(Access::from),
        }
    }
}

impl Account {
    /// owner/name of the fork's original, for a fork.
    pub fn upstream(&self) -> Option<String> {
        self.parent.as_ref().map(Access::full_name)
    }

    /// The account's access in the repository a PR or issue (by url) belongs to.
    pub fn access_for(&self, url: &str) -> Option<&Access> {
        let repo = repo_of(url).to_lowercase();
        [self.origin.as_ref(), self.parent.as_ref()]
            .into_iter()
            .flatten()
            .find(|a| a.full_name().to_lowercase() == repo)
    }

    pub fn is_origin(&self, access: &Access) -> bool {
        self.origin.as_ref() == Some(access)
    }
}

/// owner/name from a PR's or issue's html url.
pub fn repo_of(url: &str) -> String {
    let rest = url.strip_prefix("https://github.com/").unwrap_or(url);
    rest.split('/').take(2).collect::<Vec<_>>().join("/")
}

/// A check run or a commit status on a commit.
#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub name: String,
    /// "success" | "failure" | "pending" | "neutral" | "skipped" | "cancelled" | …
    pub state: String,
    /// What a pending one waits on: "queued" | "in_progress" | "waiting" | …; "" once done.
    pub status: String,
    pub description: String,
    pub app: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub url: Option<String>,
    /// A check run's id, for why it failed; a commit status has none.
    pub id: Option<u64>,
}

impl From<&github::Check> for Check {
    fn from(c: &github::Check) -> Self {
        Check {
            name: c.name.clone(),
            state: c.state.clone(),
            status: c.status.clone(),
            description: c.description.clone(),
            app: c.app.clone(),
            started_at: c.started_at.clone(),
            completed_at: c.completed_at.clone(),
            url: c.url.clone(),
            id: c.id,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommitChecks {
    pub checks: Vec<Check>,
    /// Some couldn't be read (a token without access, say): `checks` is then partial.
    pub checks_error: Option<String>,
}

impl From<&github::CommitChecks> for CommitChecks {
    fn from(c: &github::CommitChecks) -> Self {
        CommitChecks {
            checks: c.checks.iter().map(Check::from).collect(),
            checks_error: c.checks_error.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub open: u64,
    pub closed: u64,
}

impl From<&github::StateCounts> for Counts {
    fn from(c: &github::StateCounts) -> Self {
        Counts {
            open: c.open,
            closed: c.closed,
        }
    }
}

/// An ISO 8601 time from GitHub as Unix seconds.
pub fn unix(iso: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|t| t.timestamp())
}

/// Whether `err` is core's "no GitHub login found".
pub fn is_not_connected(err: &str) -> bool {
    err == github::NOT_CONNECTED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_of_reads_owner_and_name() {
        assert_eq!(repo_of("https://github.com/o/r/pull/7"), "o/r");
        assert_eq!(repo_of("https://github.com/o/r/issues/12"), "o/r");
    }

    #[test]
    fn only_a_plain_hex_color_shows() {
        let l = |c: &str| LabelItem {
            name: "x".into(),
            color: c.into(),
            description: String::new(),
        };
        assert_eq!(l("ff0000").rgb(), Some(0xff0000));
        assert_eq!(l("red").rgb(), None);
        assert_eq!(l("ff00001").rgb(), None);
    }

    #[test]
    fn github_times_read() {
        assert_eq!(unix("1970-01-01T00:01:00Z"), Some(60));
        assert_eq!(unix("garbage"), None);
    }

    #[test]
    fn a_pull_goes_back_to_core_as_it_came() {
        let core = github::Pull {
            number: 7,
            title: "T".into(),
            state: "merged".into(),
            draft: true,
            author: "a".into(),
            head_ref: "h".into(),
            head_sha: "1".into(),
            head_repo: Some("o/f".into()),
            base_ref: "main".into(),
            base_sha: "2".into(),
            created_at: "c".into(),
            updated_at: "u".into(),
            url: "https://github.com/o/r/pull/7".into(),
        };
        let back = PullItem::from(&core).to_core();
        assert_eq!(
            serde_json::to_value(&back).unwrap(),
            serde_json::to_value(&core).unwrap()
        );
    }
}
