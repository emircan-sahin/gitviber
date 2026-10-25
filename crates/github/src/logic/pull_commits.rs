//! A PR's commits as its page lists them (by day, oldest first), and which of them Files changed
//! is narrowed to.

use gitviber_core::git::Commit;

/// Picked commits: from `anchor` (the last plain click) to `to`, either way round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitRun {
    pub anchor: String,
    pub to: String,
}

/// The commits `run` covers (oldest first), as indexes; None when a force-push took either end
/// away, or when they aren't one line of first parents: a merged-in branch's commits sit next to
/// the branch's own in the list, and a diff from the first one's parent to the last would show
/// only some of them.
pub fn picked_commits(
    commits: &[Commit],
    run: Option<&CommitRun>,
) -> Option<std::ops::Range<usize>> {
    let run = run?;
    let a = commits.iter().position(|c| c.sha == run.anchor)?;
    let b = commits.iter().position(|c| c.sha == run.to)?;
    let range = a.min(b)..a.max(b) + 1;
    let list = &commits[range.clone()];
    list.windows(2)
        .all(|w| w[1].parents.first() == Some(&w[0].sha))
        .then_some(range)
}

/// Whether `picked` is every commit from the PR's merge base `base` to its head: the whole PR,
/// not a narrowing of it.
pub fn is_whole_pr(commits: &[Commit], picked: std::ops::Range<usize>, base: Option<&str>) -> bool {
    !picked.is_empty()
        && commits[picked.start].parents.first().map(String::as_str) == base
        && base.is_some()
        && picked.end == commits.len()
}

/// The run from `anchor` to `to`, or `to` alone when that run couldn't be picked.
pub fn run_to(commits: &[Commit], anchor: Option<&str>, to: &str) -> CommitRun {
    let run = CommitRun {
        anchor: anchor.unwrap_or(to).to_string(),
        to: to.to_string(),
    };
    if picked_commits(commits, Some(&run)).is_some() {
        run
    } else {
        CommitRun {
            anchor: to.into(),
            to: to.into(),
        }
    }
}

/// What a picked run of commits is called: its short id, or its first and last.
pub fn pick_label(picked: &[Commit]) -> String {
    match picked {
        [] => String::new(),
        [one] => one.short_sha.clone(),
        [first, .., last] => format!("{}–{}", first.short_sha, last.short_sha),
    }
}

/// The day a commit landed on the branch (its commit time), in local time.
pub fn commit_day(c: &Commit) -> String {
    use chrono::TimeZone as _;
    match chrono::Local.timestamp_opt(c.committed_at, 0) {
        chrono::LocalResult::Single(t) => t.format("%b %-d, %Y").to_string(),
        _ => String::new(),
    }
}

/// Runs of commits that landed on the branch the same day, in the list's order: (day, indexes).
pub fn by_day(commits: &[Commit]) -> Vec<(String, std::ops::Range<usize>)> {
    let mut days: Vec<(String, std::ops::Range<usize>)> = Vec::new();
    for (i, c) in commits.iter().enumerate() {
        let day = commit_day(c);
        match days.last_mut() {
            Some((d, range)) if *d == day => range.end = i + 1,
            _ => days.push((day, i..i + 1)),
        }
    }
    days
}

#[cfg(test)]
pub(crate) fn test_commit(sha: &str, parent: Option<&str>, at: i64) -> Commit {
    Commit {
        sha: sha.into(),
        short_sha: sha.chars().take(7).collect(),
        author_name: String::new(),
        author_email: String::new(),
        timestamp: at,
        committer_name: String::new(),
        committed_at: at,
        parents: parent.map(|p| vec![p.to_string()]).unwrap_or_default(),
        refs: vec![],
        subject: sha.into(),
        body: String::new(),
        unpushed: false,
        on_origin: true,
        not_in_head: false,
        file: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        chrono::Local
            .with_ymd_and_hms(y, mo, d, h, mi, 0)
            .single()
            .unwrap()
            .timestamp()
    }

    fn commit(sha: &str, parent: Option<&str>) -> Commit {
        test_commit(sha, parent, local(2026, 9, 28, 12, 0))
    }

    /// base ← a ← b ← c, oldest first as the PR lists them.
    fn list() -> Vec<Commit> {
        vec![
            commit("aaaaaaa1", Some("base0000")),
            commit("bbbbbbb2", Some("aaaaaaa1")),
            commit("ccccccc3", Some("bbbbbbb2")),
        ]
    }

    fn run(anchor: &str, to: &str) -> CommitRun {
        CommitRun {
            anchor: anchor.into(),
            to: to.into(),
        }
    }

    #[test]
    fn commits_either_side_of_local_midnight_fall_on_two_days() {
        let list = [
            test_commit("late", None, local(2026, 9, 27, 23, 59)),
            test_commit("early", Some("late"), local(2026, 9, 28, 0, 1)),
            test_commit("later", Some("early"), local(2026, 9, 28, 9, 30)),
        ];
        assert_eq!(
            by_day(&list),
            vec![(commit_day(&list[0]), 0..1), (commit_day(&list[1]), 1..3)]
        );
        assert_ne!(commit_day(&list[0]), commit_day(&list[1]));
    }

    #[test]
    fn a_pick_is_named_by_its_short_id_a_run_by_its_first_and_last() {
        let l = list();
        assert_eq!(pick_label(&l[1..2]), "bbbbbbb");
        assert_eq!(pick_label(&l), "aaaaaaa–ccccccc");
    }

    #[test]
    fn a_run_is_the_same_picked_either_way_round() {
        let l = list();
        assert_eq!(
            picked_commits(&l, Some(&run("aaaaaaa1", "bbbbbbb2"))),
            Some(0..2)
        );
        assert_eq!(
            picked_commits(&l, Some(&run("ccccccc3", "bbbbbbb2"))),
            Some(1..3)
        );
        assert_eq!(
            picked_commits(&l, Some(&run("bbbbbbb2", "bbbbbbb2"))),
            Some(1..2)
        );
        assert_eq!(picked_commits(&l, None), None);
    }

    #[test]
    fn a_force_push_that_took_an_end_away_drops_the_pick() {
        let l = list();
        assert_eq!(picked_commits(&l, Some(&run("gone", "bbbbbbb2"))), None);
        assert_eq!(picked_commits(&l, Some(&run("aaaaaaa1", "gone"))), None);
    }

    #[test]
    fn a_run_across_a_merged_in_branch_isnt_picked() {
        // a ← b ← M, with f1 ← f2 branched off a and merged in by M; oldest first.
        let (a, b, f1, f2, m) = ("aaaaaaa1", "bbbbbbb2", "f1f1f1f", "f2f2f2f", "mmmmmmm");
        let mut merge = commit(m, Some(b));
        merge.parents.push(f2.into());
        let merged = vec![
            commit(a, Some("base0000")),
            commit(b, Some(a)),
            commit(f1, Some(a)),
            commit(f2, Some(f1)),
            merge,
        ];
        assert_eq!(picked_commits(&merged, Some(&run(b, f1))), None);
        assert_eq!(picked_commits(&merged, Some(&run(f2, m))), None);
        assert_eq!(picked_commits(&merged, Some(&run(f1, f2))), Some(2..4));
        assert_eq!(run_to(&merged, Some(b), f1), run(f1, f1));
        assert_eq!(run_to(&merged, Some(a), b), run(a, b));
        assert_eq!(run_to(&merged, None, m), run(m, m));
    }

    #[test]
    fn picking_every_commit_from_the_merge_base_is_the_whole_pr() {
        let l = list();
        assert!(is_whole_pr(&l, 0..3, Some("base0000")));
        assert!(!is_whole_pr(&l, 0..2, Some("base0000")));
        assert!(!is_whole_pr(&l, 1..3, Some("base0000")));
        // The list capped to the newest: its first commit's parent isn't the merge base.
        assert!(!is_whole_pr(&l, 0..3, Some("older00")));
    }
}
