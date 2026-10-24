//! What the GitHub views remember between sessions, in github.json beside the settings: each
//! repository's list chips, the commits of a PR already looked at, the gh account a project
//! uses, whether git's credential store may be asked, and the issue a branch was started for.
//! Each is capped; an unreadable file starts fresh.

use std::collections::BTreeMap;
use std::path::PathBuf;

use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::logic::narrow::{Choice, ListKind, parse_choice, with_choice};

/// Repositories whose chips are kept, the most recently changed.
const NARROW_REPOS: usize = 50;
/// PRs whose seen commits are kept, the most recently opened.
const SEEN_PULLS: usize = 50;
/// Branches per repository remembered as started from an issue.
const ISSUE_BRANCHES: usize = 50;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Saved {
    /// By the repository's main worktree, oldest first: {pulls: {scope, draft}, issues: {..}}.
    narrow: Vec<(String, Value)>,
    /// By PR url, oldest first.
    seen_commits: Vec<(String, Vec<String>)>,
    /// By project (its main worktree): the gh login picked for it.
    accounts: BTreeMap<String, String>,
    /// The user connected through git's credential store: it's asked once a session by itself.
    allow_store: bool,
    /// By origin's owner/name in lowercase: (branch, issue url), oldest first.
    branch_issues: BTreeMap<String, Vec<(String, String)>>,
}

/// `value` under `key`, last in `list`, which keeps the `cap` latest.
fn put_recent<V>(list: &mut Vec<(String, V)>, key: &str, value: V, cap: usize) {
    list.retain(|(k, _)| k != key);
    list.push((key.to_string(), value));
    let over = list.len().saturating_sub(cap);
    list.drain(..over);
}

impl Saved {
    pub fn choice(&self, repo: &str, kind: ListKind) -> Choice {
        parse_choice(
            self.narrow.iter().find(|(k, _)| k == repo).map(|(_, v)| v),
            kind,
        )
    }

    /// The labels and author `kind`'s list was last narrowed to in `repo`.
    pub fn picked(
        &self,
        repo: &str,
        kind: ListKind,
    ) -> (Vec<crate::model::LabelItem>, Option<String>) {
        crate::logic::narrow::parse_picked(
            self.narrow.iter().find(|(k, _)| k == repo).map(|(_, v)| v),
            kind,
        )
    }

    pub fn set_picked(
        &mut self,
        repo: &str,
        kind: ListKind,
        labels: &[crate::model::LabelItem],
        author: Option<&str>,
    ) {
        let saved = self
            .narrow
            .iter()
            .find(|(k, _)| k == repo)
            .map(|(_, v)| v.clone());
        let next = crate::logic::narrow::with_picked(saved.as_ref(), kind, labels, author);
        put_recent(&mut self.narrow, repo, next, NARROW_REPOS);
    }

    pub fn set_choice(&mut self, repo: &str, kind: ListKind, choice: Choice) {
        let saved = self
            .narrow
            .iter()
            .find(|(k, _)| k == repo)
            .map(|(_, v)| v.clone());
        put_recent(
            &mut self.narrow,
            repo,
            with_choice(saved.as_ref(), kind, choice),
            NARROW_REPOS,
        );
    }

    /// The commits of the PR at `url` already seen; None for a PR never opened.
    pub fn seen_commits(&self, url: &str) -> Option<Vec<String>> {
        self.seen_commits
            .iter()
            .find(|(k, _)| k == url)
            .map(|(_, v)| v.clone())
    }

    /// What's `seen` of the PR's commits now (`shas`): those a force-push replaced are left behind.
    pub fn set_seen_commits(
        &mut self,
        url: &str,
        shas: &[String],
        seen: &std::collections::HashSet<String>,
    ) {
        let kept: Vec<String> = shas.iter().filter(|s| seen.contains(*s)).cloned().collect();
        put_recent(&mut self.seen_commits, url, kept, SEEN_PULLS);
    }

    pub fn account(&self, main: &str) -> Option<String> {
        self.accounts.get(main).cloned()
    }

    /// None goes back to gh's active account.
    pub fn set_account(&mut self, main: &str, login: Option<String>) {
        match login {
            Some(l) => self.accounts.insert(main.to_string(), l),
            None => self.accounts.remove(main),
        };
    }

    pub fn allow_store(&self) -> bool {
        self.allow_store
    }

    pub fn set_allow_store(&mut self, on: bool) {
        self.allow_store = on;
    }

    /// The issue (its url) `branch` was started for, by origin's owner/name: a PR from it closes it.
    pub fn branch_issue(&self, repo: &str, branch: &str) -> Option<String> {
        self.branch_issues
            .get(&repo.to_lowercase())?
            .iter()
            .find(|(b, _)| b == branch)
            .map(|(_, u)| u.clone())
    }

    pub fn set_branch_issue(&mut self, repo: &str, branch: &str, url: &str) {
        let list = self.branch_issues.entry(repo.to_lowercase()).or_default();
        put_recent(list, branch, url.to_string(), ISSUE_BRANCHES);
    }
}

/// `list`'s `cap` last.
fn keep_last<T>(mut list: Vec<T>, cap: usize) -> Vec<T> {
    let over = list.len().saturating_sub(cap);
    list.drain(..over);
    list
}

impl Saved {
    /// The old app's (its first launch here), under the same keys and oldest first as it kept
    /// them: chips, seen commits, accounts, the credential store, issue branches.
    fn from_legacy(value: impl Fn(&str) -> Option<Value>) -> Saved {
        let records = |key: &str| match value(key) {
            Some(Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        };
        let strings = |v: &Value| -> Option<Vec<String>> {
            Some(
                v.as_array()?
                    .iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect(),
            )
        };
        let narrow = records("gitviber.github.narrow")
            .into_iter()
            .filter(|(_, v)| v.is_object())
            .collect();
        let seen = records("gitviber.seenCommits")
            .into_iter()
            .filter_map(|(url, v)| Some((url, strings(&v)?)))
            .collect();
        let branch_issues = records("gitviber.issueBranches")
            .into_iter()
            .filter_map(|(repo, v)| {
                let list: Vec<(String, String)> = v
                    .as_object()?
                    .iter()
                    .filter_map(|(b, u)| Some((b.clone(), u.as_str()?.to_string())))
                    .collect();
                (!list.is_empty()).then(|| (repo.to_lowercase(), keep_last(list, ISSUE_BRANCHES)))
            })
            .collect();
        Saved {
            narrow: keep_last(narrow, NARROW_REPOS),
            seen_commits: keep_last(seen, SEEN_PULLS),
            accounts: records("gitviber.githubAccounts")
                .into_iter()
                .filter_map(|(main, v)| Some((main, v.as_str()?.to_string())))
                .collect(),
            allow_store: value("gitviber.githubStore") == Some(Value::Bool(true)),
            branch_issues,
        }
    }
}

pub struct Store(Saved);

impl Global for Store {}

pub fn path() -> PathBuf {
    gitviber_ui::store::config_dir().join("github.json")
}

pub(crate) fn init(cx: &mut App) {
    let text = std::fs::read_to_string(path()).unwrap_or_default();
    cx.set_global(Store(serde_json::from_str(&text).unwrap_or_default()));
    // The first launch: what the old app kept, unless this build has a file of its own.
    let old = Saved::from_legacy(gitviber_ui::legacy::value);
    if old != Saved::default() && !path().exists() {
        Store::update(cx, |s| *s = old);
    }
}

impl Store {
    pub fn get(cx: &App) -> &Saved {
        &cx.global::<Store>().0
    }

    /// Changes what's remembered and writes the file off the main thread.
    pub fn update(cx: &mut App, f: impl FnOnce(&mut Saved)) {
        let before = Store::get(cx).clone();
        let mut next = before.clone();
        f(&mut next);
        if next == before {
            return;
        }
        let text = serde_json::to_string_pretty(&next).unwrap_or_default();
        cx.set_global(Store(next));
        gitviber_ui::store::write_in_background(cx, path(), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::narrow::Scope;
    use std::collections::HashSet;

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_pr_never_opened_has_no_seen_commits_one_opened_keeps_those_still_in_it() {
        let mut s = Saved::default();
        assert_eq!(s.seen_commits("pr/1"), None);
        s.set_seen_commits("pr/1", &["a".into(), "b".into()], &set(&["a", "b"]));
        assert_eq!(s.seen_commits("pr/1"), Some(vec!["a".into(), "b".into()]));
        // A force-push replaced b with c: b is left behind, c isn't seen yet.
        s.set_seen_commits("pr/1", &["a".into(), "c".into()], &set(&["a", "b"]));
        assert_eq!(s.seen_commits("pr/1"), Some(vec!["a".into()]));
    }

    #[test]
    fn only_the_50_most_recently_opened_prs_are_kept() {
        let mut s = Saved::default();
        s.set_seen_commits("pr/1", &["a".into()], &set(&["a"]));
        for i in 0..51 {
            s.set_seen_commits(&format!("pr/{i}"), &["a".into()], &set(&["a"]));
        }
        // pr/1 was saved before, and again in the loop: pr/0 is the oldest now.
        assert_eq!(s.seen_commits("pr/0"), None);
        assert!(s.seen_commits("pr/1").is_some());
        assert!(s.seen_commits("pr/50").is_some());
    }

    #[test]
    fn chips_are_kept_per_repository_and_kind() {
        let mut s = Saved::default();
        let created = Choice {
            scope: Some(Scope::Created),
            draft: Some(true),
        };
        s.set_choice("/repo", ListKind::Pulls, created);
        s.set_choice(
            "/repo",
            ListKind::Issues,
            Choice {
                scope: Some(Scope::Mentioned),
                draft: None,
            },
        );
        assert_eq!(s.choice("/repo", ListKind::Pulls), created);
        assert_eq!(
            s.choice("/repo", ListKind::Issues).scope,
            Some(Scope::Mentioned)
        );
        assert_eq!(s.choice("/other", ListKind::Pulls), Choice::default());
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Saved>(&text).unwrap(), s);
        assert_eq!(serde_json::from_str::<Saved>("{\"narrow\": 3}").ok(), None);
    }

    #[test]
    fn the_old_apps_records_come_over_in_their_order() {
        let old = serde_json::json!({
            "gitviber.github.narrow": {"/p/b": {"pulls": {"scope": "created", "draft": true}}, "/p/a": {"issues": {"scope": null}}, "/p/x": 3},
            "gitviber.seenCommits": {"pr/2": ["a", 1, "b"], "pr/1": ["c"]},
            "gitviber.githubAccounts": {"/p/a": "mona", "/p/b": null},
            "gitviber.githubStore": true,
            "gitviber.issueBranches": {"O/R": {"12-fix": "https://github.com/o/r/issues/12"}},
        });
        let s = Saved::from_legacy(|key| old.get(key).cloned());
        let repos: Vec<&str> = s.narrow.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(repos, ["/p/b", "/p/a"]);
        assert_eq!(
            s.choice("/p/b", ListKind::Pulls).scope,
            Some(Scope::Created)
        );
        assert_eq!(s.choice("/p/b", ListKind::Pulls).draft, Some(true));
        assert_eq!(s.seen_commits("pr/2"), Some(vec!["a".into(), "b".into()]));
        assert_eq!(s.seen_commits[1].0, "pr/1");
        assert_eq!(s.account("/p/a").as_deref(), Some("mona"));
        assert_eq!(s.account("/p/b"), None);
        assert!(s.allow_store());
        assert_eq!(
            s.branch_issue("o/r", "12-fix").as_deref(),
            Some("https://github.com/o/r/issues/12")
        );
        assert_eq!(Saved::from_legacy(|_| None), Saved::default());
    }

    #[test]
    fn a_branch_remembers_the_issue_it_was_started_for() {
        let mut s = Saved::default();
        s.set_branch_issue("O/R", "12-fix", "https://github.com/o/r/issues/12");
        assert_eq!(
            s.branch_issue("o/r", "12-fix").as_deref(),
            Some("https://github.com/o/r/issues/12")
        );
        assert_eq!(s.branch_issue("o/r", "main"), None);
        s.set_account("/main", Some("mona".into()));
        assert_eq!(s.account("/main").as_deref(), Some("mona"));
        s.set_account("/main", None);
        assert_eq!(s.account("/main"), None);
    }
}
