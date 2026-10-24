//! The pull request a branch's badge shows, out of every cached list.

use std::collections::HashMap;

use crate::model::{PullItem, PullState};

/// One whose head is `branch` of `head_repo` (another fork's same-named branch isn't it). Open
/// beats merged beats closed, then the most recently updated. The same PR may sit in several
/// lists, an older copy still open: the newest copy of each counts. With the branch's `tip` (a
/// short id), a closed or merged PR must end there: a name reused since isn't its branch.
pub fn pull_for_branch<'a>(
    pulls: impl IntoIterator<Item = &'a PullItem>,
    branch: &str,
    head_repo: &str,
    tip: Option<&str>,
) -> Option<&'a PullItem> {
    let repo = head_repo.to_lowercase();
    let mut newest: HashMap<&str, &PullItem> = HashMap::new();
    for p in pulls {
        if p.head_ref != branch
            || p.head_repo.as_deref().map(str::to_lowercase).as_deref() != Some(repo.as_str())
        {
            continue;
        }
        match newest.get(p.url.as_str()) {
            Some(seen) if seen.updated_at >= p.updated_at => {}
            _ => {
                newest.insert(&p.url, p);
            }
        }
    }
    let mut best: Option<&PullItem> = None;
    // By url first, so a tie lands the same way every time.
    let mut list: Vec<&PullItem> = newest.into_values().collect();
    list.sort_by(|a, b| a.url.cmp(&b.url));
    for p in list {
        if let Some(tip) = tip.filter(|t| !t.is_empty())
            && p.state != PullState::Open
            && !p.head_sha.starts_with(tip)
        {
            continue;
        }
        let better = match best {
            None => true,
            Some(b) => {
                p.state.rank() > b.state.rank()
                    || (p.state.rank() == b.state.rank() && p.updated_at > b.updated_at)
            }
        };
        if better {
            best = Some(p);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(number: u64, state: PullState, updated_at: &str) -> PullItem {
        pr_on(
            number,
            state,
            updated_at,
            "fix",
            Some("me/app"),
            "abc1234def",
        )
    }

    fn pr_on(
        number: u64,
        state: PullState,
        updated_at: &str,
        head_ref: &str,
        head_repo: Option<&str>,
        head_sha: &str,
    ) -> PullItem {
        PullItem {
            number,
            title: String::new(),
            state,
            draft: false,
            author: "me".into(),
            head_ref: head_ref.into(),
            head_sha: head_sha.into(),
            head_repo: head_repo.map(str::to_string),
            base_ref: "main".into(),
            base_sha: String::new(),
            created_at: updated_at.into(),
            updated_at: updated_at.into(),
            url: format!("https://github.com/me/app/pull/{number}"),
        }
    }

    use PullState::*;

    #[test]
    fn open_beats_merged_beats_closed_whatever_was_updated_last() {
        let list = [
            pr(1, Closed, "2026-03-01"),
            pr(2, Merged, "2026-02-01"),
            pr(3, Open, "2026-01-01"),
        ];
        assert_eq!(
            pull_for_branch(&list, "fix", "me/app", None).map(|p| p.number),
            Some(3)
        );
        assert_eq!(
            pull_for_branch(&list[..2], "fix", "me/app", None).map(|p| p.number),
            Some(2)
        );
    }

    #[test]
    fn among_the_same_state_the_most_recently_updated() {
        let list = [pr(1, Merged, "2026-01-01"), pr(2, Merged, "2026-02-01")];
        assert_eq!(
            pull_for_branch(&list, "fix", "me/app", None).map(|p| p.number),
            Some(2)
        );
    }

    #[test]
    fn only_that_branch_of_that_repository_ignoring_case() {
        let list = [
            pr_on(1, Open, "2026-01-01", "fix", Some("other/app"), "abc"),
            pr_on(2, Open, "2026-01-01", "fix-2", Some("me/app"), "abc"),
            pr_on(3, Open, "2026-01-01", "fix", None, "abc"),
        ];
        assert!(pull_for_branch(&list, "fix", "me/app", None).is_none());
        let upper = [pr_on(4, Open, "2026-01-01", "fix", Some("Me/App"), "abc")];
        assert_eq!(
            pull_for_branch(&upper, "fix", "me/app", None).map(|p| p.number),
            Some(4)
        );
    }

    #[test]
    fn a_lists_stale_open_copy_loses_to_a_newer_merged_one() {
        let list = [pr(5, Open, "2026-01-01"), pr(5, Merged, "2026-02-01")];
        assert_eq!(
            pull_for_branch(&list, "fix", "me/app", None).map(|p| p.state),
            Some(Merged)
        );
    }

    #[test]
    fn a_closed_or_merged_pr_must_end_at_the_branchs_tip() {
        let list = [pr_on(
            1,
            Merged,
            "2026-01-01",
            "fix",
            Some("me/app"),
            "0000000aaa",
        )];
        assert!(pull_for_branch(&list, "fix", "me/app", Some("abc1234")).is_none());
        assert_eq!(
            pull_for_branch(&list, "fix", "me/app", Some("0000000")).map(|p| p.number),
            Some(1)
        );
        assert_eq!(
            pull_for_branch(&list, "fix", "me/app", None).map(|p| p.number),
            Some(1)
        );
        let open = [pr(2, Open, "2026-01-01")];
        assert_eq!(
            pull_for_branch(&open, "fix", "me/app", Some("fffffff")).map(|p| p.number),
            Some(2)
        );
    }
}
