use super::*;
use crate::git;
use crate::network::Net;
use std::path::Path;

/// Read-only, against this checkout's origin: `cargo test -- --ignored live_label_filter`.
#[test]
#[ignore = "talks to GitHub"]
fn live_label_filter() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let session = Session::default();
    let labels = issue_labels(&session, repo, None).unwrap();
    println!("{} labels", labels.len());
    // Names with spaces and colons are the ones that need encoding.
    let odd = labels
        .iter()
        .find(|l| l.name.contains(' '))
        .expect("a label with a space");
    let with = issues(&session, repo, None, "all", std::slice::from_ref(&odd.name)).unwrap();
    println!("{:?}: {} issues", odd.name, with.len());
    assert!(!with.is_empty());
    assert!(with
        .iter()
        .all(|i| i.labels.iter().any(|l| l.name == odd.name)));
    // The list stops at 50; the counts don't.
    let c = issue_counts(&session, repo, None, std::slice::from_ref(&odd.name)).unwrap();
    println!("counted {} open, {} closed", c.open, c.closed);
    let counted = (c.open + c.closed) as usize;
    assert!(counted >= with.len() && (with.len() == 50 || counted == with.len()));
    let all = issue_counts(&session, repo, None, &[]).unwrap();
    assert!(all.open + all.closed >= c.open + c.closed);
    // Two labels mean both: never more than either alone.
    let other = &with[0].labels.iter().find(|l| l.name != odd.name);
    if let Some(other) = other {
        let both = issues(
            &session,
            repo,
            None,
            "all",
            &[odd.name.clone(), other.name.clone()],
        )
        .unwrap();
        println!("+ {:?}: {} issues", other.name, both.len());
        assert!(!both.is_empty() && both.len() <= with.len());
        assert!(both.iter().all(|i| i.labels.len() >= 2));
    }
}

/// Read-only, against a clone of a fork: `GITVIBER_GH_FORK=/path/to/clone cargo test -- --ignored`.
#[test]
#[ignore = "talks to GitHub"]
fn live_fork_reads_both_repositories() {
    let path = std::env::var("GITVIBER_GH_FORK").expect("GITVIBER_GH_FORK");
    let repo = Path::new(&path);
    let session = Session::default();
    let acct = account(&session, repo).unwrap();
    let origin = acct.origin.expect("origin");
    let parent = acct.parent.expect("origin should be a fork");
    println!(
        "origin {} push={} issues={} · parent {} push={} admin={} issues={} default={:?}",
        origin.repo.full(),
        origin.push,
        origin.issues,
        parent.repo.full(),
        parent.push,
        parent.admin,
        parent.issues,
        parent.default_branch
    );
    let up = parent.repo.full();
    for (to, name) in [(None, "origin"), (Some(up.as_str()), "parent")] {
        let pulls = list(&session, repo, to, "all", 1).unwrap();
        let open = issues(&session, repo, to, "open", &[]).unwrap_or_default();
        let all = issues(&session, repo, to, "all", &[]).unwrap_or_default();
        println!(
            "{name}: {} PRs, {} open / {} total issues",
            pulls.len(),
            open.len(),
            all.len()
        );
        // "all" must never list fewer than "open": PRs used to crowd issues out of the page.
        assert!(all.len() >= open.len().min(50));
        if let Some(p) = pulls.first() {
            assert!(p.url.to_lowercase().contains(&format!(
                "/{}/pull/",
                if to.is_some() {
                    up.to_lowercase()
                } else {
                    origin.repo.full().to_lowercase()
                }
            )));
            detail(&session, repo, to, p.number).unwrap();
        }
    }
    let closed = list(&session, repo, Some(&up), "closed", 1).unwrap();
    if let Some(p) = closed.iter().find(|p| p.state == "closed") {
        let d = detail(&session, repo, Some(&up), p.number).unwrap();
        println!("#{} closed by {:?}", p.number, d.closed_by);
        assert!(d.closed_by.is_some());
    }
    // Anything but origin and its parent is refused, whatever the token could reach.
    assert!(list(&session, repo, Some("torvalds/linux"), "open", 1).is_err());
    let remote = original_remote(repo, &up, false, &Net::default()).unwrap();
    println!("original remote: {remote:?}");
    if let Some(r) = remote {
        let branch = parent.default_branch.unwrap_or_else(|| "main".into());
        let log = git::log(repo, Some(&format!("refs/remotes/{r}/{branch}")), 0, 20).unwrap();
        println!(
            "{} commits on {r}/{branch}, {} not in HEAD",
            log.len(),
            log.iter().filter(|c| c.not_in_head).count()
        );
    }
}

/// End to end against a real repo: `GITVIBER_GH_REPO=/path/to/clone cargo test -- --ignored`.
/// The clone's origin needs a `feature/review` branch that differs from `main`.
#[test]
#[ignore = "talks to GitHub"]
fn live_pull_request_flow() {
    let path = std::env::var("GITVIBER_GH_REPO").expect("GITVIBER_GH_REPO");
    let repo = Path::new(&path);
    let session = Session::default();
    let acct = account(&session, repo).unwrap();
    println!(
        "account: {} via {}, default branch {:?}",
        acct.login,
        acct.source,
        acct.origin.and_then(|o| o.default_branch)
    );

    let open = list(&session, repo, None, "open", 1).unwrap();
    let number = match open.iter().find(|p| p.head_ref == "feature/review") {
        Some(p) => p.number,
        None => {
            create(
                &session,
                repo,
                None,
                "Review: newest commit opens by default",
                "Opened by GitViber's live test.",
                "feature/review",
                "main",
                false,
                false,
            )
            .unwrap()
            .number
        }
    };
    let d = detail(&session, repo, None, number).unwrap();
    println!(
        "PR #{number}: {} ({}), mergeable {:?}, {} checks, {} comments",
        d.pull.title,
        d.pull.state,
        d.mergeable,
        d.checks.len(),
        d.comments.len()
    );
    assert_eq!(d.pull.head_ref, "feature/review");

    let f = files(
        &session,
        repo,
        None,
        number,
        &d.pull.base_ref,
        &d.pull.base_sha,
        &d.pull.head_sha,
        &Net::default(),
    )
    .unwrap();
    println!(
        "files: {:?}",
        f.files
            .iter()
            .map(|x| (&x.path, x.additions, x.deletions))
            .collect::<Vec<_>>()
    );
    assert_eq!(f.files.len() as u64, d.changed_files);
    let pair = git::diff_pair(
        repo,
        "range",
        &f.files[0].path,
        None,
        Some(&f.head),
        Some(&f.base),
        None,
        |_| git::FileText::default(),
    )
    .unwrap();
    assert!(pair.rows.iter().any(|r| r.k != 0));
}

/// The "Resolve locally" flow on a PR that conflicts with its base (diff-demo PR #2).
/// Local only: aborts at the end and never pushes.
#[test]
#[ignore = "talks to GitHub"]
fn live_resolve_locally() {
    let path = std::env::var("GITVIBER_GH_REPO").expect("GITVIBER_GH_REPO");
    let repo = Path::new(&path);
    let session = Session::default();
    let d = detail(&session, repo, None, 2).unwrap();
    println!(
        "PR #2 mergeable={:?} state={}",
        d.mergeable, d.mergeable_state
    );
    let same_repo = d.pull.head_repo.as_deref()
        == Some(
            &*d.pull
                .url
                .replace("https://github.com/", "")
                .split("/pull/")
                .next()
                .unwrap()
                .to_string(),
        );
    checkout(
        repo,
        "origin",
        None,
        2,
        &d.pull.head_ref,
        same_repo,
        &Net::default(),
    )
    .unwrap();
    assert_eq!(
        git::status(repo).unwrap().branch.as_deref(),
        Some(d.pull.head_ref.as_str())
    );
    git::fetch(repo, &Default::default()).unwrap();
    let stopped = git::merge(
        repo,
        &format!("origin/{}", d.pull.base_ref),
        git::MergeKind::Ff,
    )
    .unwrap();
    let st = git::status(repo).unwrap();
    println!(
        "stopped={stopped} conflicts={:?}",
        st.conflicted
            .iter()
            .map(|f| (&f.path, &f.conflict))
            .collect::<Vec<_>>()
    );
    assert!(stopped && st.conflicted.len() == 1);
    git::op_abort(repo).unwrap();
    git::switch_branch(repo, "main", false).unwrap();
}
