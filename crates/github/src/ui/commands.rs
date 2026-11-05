//! The palette's pull request commands: Create Pull Request… and Open Pull Request for This
//! Branch. On a window's root while its origin is on GitHub; each says why when it can't run,
//! in the words of the PRs panel's New button.

use gitviber_core::api;
use gitviber_core::git::RepoStatus;
use gitviber_ui::actions::github;
use gitviber_ui::workspace::{Toast, Workspace};
use gpui_kit::{Action, App, Entity, Global, InteractiveElement, Subscription, Window};

use crate::cache::{Cache, MIN_AGE};
use crate::model::{Access, PullItem, PullState, repo_of};
use crate::ui::common::github_failure;
use crate::ui::dialogs::CreatePullDialog;
use crate::ui::pulls_panel::{
    BranchStatus, PAGE, branch_lists, create_blocked, create_target, load_branch_lists,
};

/// Create Pull Request… and Open Pull Request for This Branch on a window's root, there only
/// while `status` says origin is on GitHub: the palette offers what something listens for.
pub fn pull_actions<E: InteractiveElement>(el: E, status: Option<&RepoStatus>) -> E {
    let Some(s) = status.filter(|s| matches!(s.web_url, Some(Some(_)))) else {
        return el;
    };
    let branch = BranchStatus::read(s.branch.clone(), s.follows.clone(), s.push.as_ref());
    let open = branch.clone();
    el.on_action(move |_: &github::CreatePull, window, cx| {
        run(Cmd::Create, branch.clone(), window, cx)
    })
    .on_action(move |_: &github::OpenBranchPull, window, cx| {
        run(Cmd::Open, open.clone(), window, cx)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmd {
    Create,
    Open,
}

impl Cmd {
    fn not_run(self) -> &'static str {
        match self {
            Cmd::Create => "Create pull request not run",
            Cmd::Open => "Open pull request not run",
        }
    }

    fn action(self) -> Box<dyn Action> {
        match self {
            Cmd::Create => Box::new(github::CreatePull),
            Cmd::Open => Box::new(github::OpenBranchPull),
        }
    }
}

/// What a command does with what the cache holds.
#[derive(Debug, PartialEq)]
enum Next {
    /// The account or the branch's PR lists are still on their way.
    Wait,
    /// Reading them failed: GitHub's error, signed out included.
    Failed(String),
    /// Why it can't run, and what fixes it.
    NotRun(String, Option<Fix>),
    Create {
        target: Access,
        origin: Access,
        status: BranchStatus,
    },
    Open(PullItem),
}

#[derive(Debug, PartialEq)]
enum Fix {
    Open(Box<PullItem>),
    Create,
}

/// The command waiting for what it reads: one at a time, so pressing again doesn't stack a
/// second dialog or toast.
#[derive(Default)]
struct Pending(Option<Subscription>);

impl Global for Pending {}

/// The account and the lists the branch badges read are asked for as a project opens; a
/// command pressed before they're in is pressed again once they are, unless another
/// repository's views took the cache meanwhile.
fn run(cmd: Cmd, status: BranchStatus, window: &mut Window, cx: &mut App) {
    let Some(ws) = Workspace::for_window(window, cx) else {
        return;
    };
    // A press replaces the one waiting.
    drop(cx.default_global::<Pending>().0.take());
    crate::account::load(cx, false);
    load_branch_lists(cx);
    let next = next(cmd, &status, cx);
    if next != Next::Wait {
        return act(cmd, next, &ws, window, cx);
    }
    let (handle, ws, generation) = (
        window.window_handle(),
        ws.downgrade(),
        Cache::generation(cx),
    );
    let waiting = cx.observe_global::<Cache>(move |cx| {
        let next = (Cache::generation(cx) == generation).then(|| self::next(cmd, &status, cx));
        if next == Some(Next::Wait) {
            return;
        }
        drop(cx.default_global::<Pending>().0.take());
        let (Some(next), Some(ws)) = (next, ws.upgrade()) else {
            return;
        };
        handle
            .update(cx, |_, window, cx| match next {
                // Told as is: pressed again, a failed account read would retry and wait again.
                Next::Failed(_) => act(cmd, next, &ws, window, cx),
                // Pressed again, so it runs for the branch checked out by now.
                _ => window.dispatch_action(cmd.action(), cx),
            })
            .ok();
    });
    cx.default_global::<Pending>().0 = Some(waiting);
}

fn next(cmd: Cmd, status: &BranchStatus, cx: &mut App) -> Next {
    let read = crate::account::current_account(cx);
    // An account read before is used through a refresh's failure, unless the login went; a
    // failure whose retry is on its way waits for that.
    let failed = read
        .error
        .as_deref()
        .filter(|e| read.data.is_none() || github_failure(e).needs_sign_in());
    let account = match (failed, read.data) {
        (None, Some(account)) => account,
        (Some(e), _) if !read.loading => return Next::Failed(e.to_string()),
        _ => return Next::Wait,
    };
    // Each list's repository (None: origin), and whether it filled its page.
    let mut full: Vec<(Option<String>, bool)> = vec![];
    for (repo, _, key) in branch_lists(cx) {
        let list = Cache::read::<Vec<PullItem>>(cx, &key);
        match (list.data, list.error) {
            (Some(rows), _) => full.push((repo, rows.len() >= PAGE)),
            (None, Some(e)) if !list.loading => return Next::Failed(e),
            (None, _) => return Next::Wait,
        }
    }
    let origin = account.origin.clone();
    let parent = account.parent.as_ref();
    let mut open = origin
        .as_ref()
        .and_then(|o| open_pull(status, o, parent, cx));
    if open.is_none()
        && status.follows.is_none()
        && let (Some(o), Some(branch)) = (&origin, &status.branch)
    {
        // A repository whose every list filled its page may hold the branch's PR past it.
        let mut repos: Vec<Option<String>> = full.iter().map(|(r, _)| r.clone()).collect();
        repos.dedup();
        repos.retain(|r| full.iter().filter(|(l, _)| l == r).all(|(_, f)| *f));
        for repo in repos {
            let read = read_head(repo, format!("{}:{branch}", o.owner), cx);
            match (read.data, read.error) {
                (Some(rows), _) => open = open.or_else(|| rows.first().cloned()),
                (None, Some(e)) if !read.loading => return Next::Failed(e),
                (None, _) => return Next::Wait,
            }
        }
    }
    let blocked = create_blocked(
        status,
        open.as_ref().map(|p| p.number),
        origin.as_ref(),
        false,
    );
    match (cmd, open) {
        (Cmd::Open, Some(p)) => Next::Open(p),
        (Cmd::Open, None) => {
            let why = match &status.branch {
                Some(b) => format!("There's no open pull request for {b}."),
                None => "Check out a branch first.".into(),
            };
            Next::NotRun(why, blocked.is_none().then_some(Fix::Create))
        }
        (Cmd::Create, open) => match (blocked, origin) {
            (Some(why), _) => Next::NotRun(format!("{why}."), open.map(|p| Fix::Open(Box::new(p)))),
            (None, Some(origin)) => Next::Create {
                target: create_target(&origin, parent, None, cx),
                origin,
                status: status.clone(),
            },
            (None, None) => unreachable!("create_blocked says origin's access is missing"),
        },
    }
}

/// The open PRs from `head` (owner:branch) into `repo` (None: origin), asked of GitHub. Kept
/// apart from the "pulls:" lists: nothing reads it again as they change, so a badge or a
/// pull request's view would take its rows for current.
fn read_head(
    repo: Option<String>,
    head: String,
    cx: &mut App,
) -> crate::cache::Read<Vec<PullItem>> {
    let key = head_key(repo.as_deref(), &head);
    Cache::load_held(cx, &key, MIN_AGE, move |core| {
        api::github::pr_open_from(core, repo.clone(), head.clone())
            .map(|ps| ps.iter().map(PullItem::from).collect::<Vec<_>>())
    });
    Cache::read(cx, &key)
}

fn head_key(repo: Option<&str>, head: &str) -> String {
    format!("head-pulls:{}:{head}", repo.unwrap_or("origin"))
}

/// The branch's open PR in the lists read: a checked-out one by its number, in origin or the
/// fork's original; else one from the branch as origin has it.
fn open_pull(
    status: &BranchStatus,
    origin: &Access,
    parent: Option<&Access>,
    cx: &App,
) -> Option<PullItem> {
    if let Some(n) = status.follows.as_deref() {
        let n: u64 = n.trim_start_matches('#').parse().ok()?;
        let repos: Vec<String> = [Some(origin), parent]
            .into_iter()
            .flatten()
            .map(|r| r.full_name().to_lowercase())
            .collect();
        return Cache::rows::<PullItem>(cx, "pulls:").into_iter().find(|p| {
            p.number == n
                && p.state == PullState::Open
                && repos.contains(&repo_of(&p.url).to_lowercase())
        });
    }
    let branch = status.branch.as_deref()?;
    crate::account::branch_pull(cx, branch, &origin.full_name(), None)
        .filter(|p| p.state == PullState::Open)
}

fn act(cmd: Cmd, next: Next, ws: &Entity<Workspace>, window: &mut Window, cx: &mut App) {
    let toast = match next {
        Next::Wait => return,
        Next::Create {
            target,
            origin,
            status,
        } => {
            let head = status.branch.clone().unwrap_or_default();
            return CreatePullDialog::open(
                ws.downgrade(),
                target,
                origin,
                head,
                status,
                window,
                cx,
            );
        }
        Next::Open(p) => return crate::ui::open_pull(&ws.downgrade(), p, false, window, cx),
        Next::Failed(e) => github_failure(&e).toast(cmd.not_run(), None),
        Next::NotRun(why, fix) => {
            let toast = Toast::info(why).title(cmd.not_run());
            match fix {
                None => toast,
                Some(Fix::Create) => toast.action("Create Pull Request…", |window, cx| {
                    window.dispatch_action(Box::new(github::CreatePull), cx)
                }),
                Some(Fix::Open(p)) => {
                    let weak = ws.downgrade();
                    toast.action(format!("Open #{}", p.number), move |window, cx| {
                        crate::ui::open_pull(&weak, (*p).clone(), false, window, cx)
                    })
                }
            }
        }
    };
    ws.update(cx, |ws, cx| ws.toast(toast, cx));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::MIN_AGE;
    use crate::leak_tests::{pull, setup};
    use crate::model::Account;
    use gpui_kit::{
        AppContext as _, Context, FocusHandle, IntoElement, Render, TestAppContext,
        VisualTestContext, div,
    };

    fn access(owner: &str) -> Access {
        Access {
            owner: owner.into(),
            name: "r".into(),
            default_branch: Some("main".into()),
            push: true,
            triage: false,
            admin: false,
            issues: true,
            allow_merge_commit: None,
            allow_squash_merge: None,
            allow_rebase_merge: None,
        }
    }

    fn account(parent: Option<Access>) -> Result<Account, String> {
        Ok(Account {
            login: "o".into(),
            source: "gh".into(),
            origin: Some(access("o")),
            parent,
        })
    }

    fn read_account(answer: Result<Account, String>, cx: &mut App) {
        Cache::load(cx, crate::account::KEY, MIN_AGE, move |_| answer.clone());
    }

    /// Every list the branch badges read answers `pulls`.
    fn read_lists(pulls: Vec<PullItem>, cx: &mut App) {
        for (.., key) in branch_lists(cx) {
            let pulls = pulls.clone();
            Cache::load(cx, &key, MIN_AGE, move |_| Ok(pulls.clone()));
        }
    }

    fn on(branch: &str) -> BranchStatus {
        BranchStatus {
            branch: Some(branch.into()),
            ..Default::default()
        }
    }

    struct Root {
        status: Option<RepoStatus>,
        focus: FocusHandle,
        _ws: Entity<Workspace>,
    }

    impl Render for Root {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            pull_actions(div().track_focus(&self.focus), self.status.as_ref())
        }
    }

    /// A window whose root has `status`, focused, with its workspace.
    fn window(
        status: Option<RepoStatus>,
        cx: &mut TestAppContext,
    ) -> (Entity<Workspace>, &mut VisualTestContext) {
        let ws = cx.update(|cx| cx.new(Workspace::new));
        let handle = cx.add_window(|_, cx| Root {
            status,
            focus: cx.focus_handle(),
            _ws: ws.clone(),
        });
        let cx = VisualTestContext::from_window(*handle, cx).into_mut();
        cx.update(|window, cx| {
            Workspace::register_window(&ws, window, cx);
            let root = window.root::<Root>().flatten().unwrap();
            let focus = root.read(cx).focus.clone();
            focus.focus(window, cx);
        });
        cx.run_until_parked();
        (ws, cx)
    }

    fn github(branch: &str) -> RepoStatus {
        RepoStatus {
            branch: Some(branch.into()),
            web_url: Some(Some("https://github.com/o/r".into())),
            ..Default::default()
        }
    }

    #[gpui_kit::test]
    fn the_commands_are_offered_only_for_a_github_repository(cx: &mut TestAppContext) {
        setup(cx);
        let offered = |status: Option<RepoStatus>, cx: &mut TestAppContext| {
            let (_, cx) = window(status, cx);
            cx.update(|window, cx| {
                (
                    window.is_action_available(&github::CreatePull, cx),
                    window.is_action_available(&github::OpenBranchPull, cx),
                )
            })
        };
        assert_eq!(offered(Some(github("fix")), cx), (true, true));
        // Origin on another host, no origin, git not saying yet, and no repository.
        let elsewhere = RepoStatus {
            web_url: Some(None),
            ..Default::default()
        };
        assert_eq!(offered(Some(elsewhere), cx), (false, false));
        assert_eq!(offered(Some(RepoStatus::default()), cx), (false, false));
        assert_eq!(offered(None, cx), (false, false));
    }

    #[gpui_kit::test]
    fn each_command_finds_the_branchs_open_pull_request(cx: &mut TestAppContext) {
        setup(cx);
        let next = |cmd: Cmd, status: BranchStatus, cx: &mut TestAppContext| {
            cx.update(|cx| next(cmd, &status, cx))
        };
        // Nothing read yet: it waits.
        assert_eq!(next(Cmd::Open, on("fix"), cx), Next::Wait);
        cx.update(|cx| read_account(account(None), cx));
        cx.run_until_parked();
        assert_eq!(next(Cmd::Open, on("fix"), cx), Next::Wait);
        let mut closed = pull(41);
        closed.state = PullState::Closed;
        closed.head_ref = "old".into();
        cx.update(|cx| read_lists(vec![pull(42), closed], cx));
        cx.run_until_parked();

        assert_eq!(next(Cmd::Open, on("fix"), cx), Next::Open(pull(42)));
        assert_eq!(
            next(Cmd::Create, on("fix"), cx),
            Next::NotRun(
                "#42 already open for this branch.".into(),
                Some(Fix::Open(Box::new(pull(42))))
            )
        );
        // A checked-out PR by its number.
        let follows = BranchStatus {
            follows: Some("#42".into()),
            ..on("pr-42")
        };
        assert_eq!(next(Cmd::Open, follows, cx), Next::Open(pull(42)));
        // A closed one isn't opened; Create is offered instead.
        assert_eq!(
            next(Cmd::Open, on("old"), cx),
            Next::NotRun(
                "There's no open pull request for old.".into(),
                Some(Fix::Create)
            )
        );
        assert_eq!(
            next(Cmd::Create, on("old"), cx),
            Next::Create {
                target: access("o"),
                origin: access("o"),
                status: on("old"),
            }
        );
        // No branch: nothing to create from either.
        let detached = BranchStatus::default();
        assert_eq!(
            next(Cmd::Open, detached.clone(), cx),
            Next::NotRun("Check out a branch first.".into(), None)
        );
        assert_eq!(
            next(Cmd::Create, detached, cx),
            Next::NotRun("Check out a branch first.".into(), None)
        );
    }

    #[gpui_kit::test]
    fn a_forks_pull_request_goes_to_its_original(cx: &mut TestAppContext) {
        setup(cx);
        cx.update(|cx| read_account(account(Some(access("up"))), cx));
        cx.run_until_parked();
        // The original's list too, once the account names it.
        cx.update(|cx| read_lists(vec![], cx));
        cx.run_until_parked();
        let next = cx.update(|cx| next(Cmd::Create, &on("fix"), cx));
        assert_eq!(
            next,
            Next::Create {
                target: access("up"),
                origin: access("o"),
                status: on("fix"),
            }
        );
    }

    /// Signed out: the command says so with Sign in…, as any GitHub call does.
    #[gpui_kit::test]
    fn signed_out_the_commands_offer_sign_in(cx: &mut TestAppContext) {
        setup(cx);
        let (ws, cx) = window(Some(github("fix")), cx);
        let gone = Err(gitviber_core::github::NOT_CONNECTED.to_string());
        cx.update(|_, cx| read_account(gone, cx));
        cx.run_until_parked();
        cx.dispatch_action(github::CreatePull);
        cx.run_until_parked();
        let toast = cx.update(|_, cx| ws.update(cx, |ws, _| ws.take_toasts()));
        let toast = toast.last().expect("a toast");
        assert_eq!(toast.title.as_deref(), Some("Create pull request not run"));
        assert_eq!(
            toast.action.as_ref().map(|a| a.label.as_ref()),
            Some("Sign in…")
        );
    }

    /// Pressed while the lists are read, the command runs once they're in: with no PR, it offers
    /// Create Pull Request….
    #[gpui_kit::test]
    fn open_waits_for_the_lists_then_offers_create(cx: &mut TestAppContext) {
        setup(cx);
        let (ws, cx) = window(Some(github("fix")), cx);
        cx.update(|_, cx| read_account(account(None), cx));
        cx.run_until_parked();
        let mut other = pull(7);
        other.head_ref = "other".into();
        cx.update(|window, cx| {
            read_lists(vec![other], cx);
            window.dispatch_action(Box::new(github::OpenBranchPull), cx);
        });
        let toasts =
            |cx: &mut VisualTestContext| cx.update(|_, cx| ws.update(cx, |ws, _| ws.take_toasts()));
        assert!(toasts(cx).is_empty());
        cx.run_until_parked();
        let toasts = toasts(cx);
        assert_eq!(toasts.len(), 1, "once");
        assert_eq!(
            toasts[0].title.as_deref(),
            Some("Open pull request not run")
        );
        assert_eq!(
            toasts[0].message.as_ref(),
            "There's no open pull request for fix."
        );
        assert_eq!(
            toasts[0].action.as_ref().map(|a| a.label.as_ref()),
            Some("Create Pull Request…")
        );
    }

    fn toasts(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<Toast> {
        cx.update(|_, cx| ws.update(cx, |ws, _| ws.take_toasts()))
    }

    /// A window with Open pressed and the lists on their way: `other`'s PR is all they hold.
    /// Nothing lands till the test parks.
    fn waiting(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
        setup(cx);
        let (ws, cx) = window(Some(github("fix")), cx);
        cx.update(|_, cx| read_account(account(None), cx));
        cx.run_until_parked();
        let mut other = pull(7);
        other.head_ref = "other".into();
        cx.update(|window, cx| {
            read_lists(vec![other], cx);
            window.dispatch_action(Box::new(github::OpenBranchPull), cx);
        });
        assert!(toasts(&ws, cx).is_empty());
        (ws, cx)
    }

    #[gpui_kit::test]
    fn pressing_again_while_it_waits_runs_it_once(cx: &mut TestAppContext) {
        let (ws, cx) = waiting(cx);
        cx.update(|window, cx| {
            window.dispatch_action(Box::new(github::OpenBranchPull), cx);
            window.dispatch_action(Box::new(github::OpenBranchPull), cx);
        });
        cx.run_until_parked();
        assert_eq!(toasts(&ws, cx).len(), 1);
    }

    /// Another repository's views took the cache: nothing comes up.
    #[gpui_kit::test]
    fn a_waiting_command_ends_with_its_repository(cx: &mut TestAppContext) {
        let (ws, cx) = waiting(cx);
        cx.update(|_, cx| {
            Cache::set_origin(cx, Some("https://github.com/o/a".into()));
            Cache::set_origin(cx, Some("https://github.com/o/b".into()));
        });
        cx.run_until_parked();
        assert!(toasts(&ws, cx).is_empty());
    }

    #[gpui_kit::test]
    fn a_waiting_command_ends_with_its_window(cx: &mut TestAppContext) {
        let (ws, cx) = waiting(cx);
        cx.update(|window, _| window.remove_window());
        let app: &mut TestAppContext = cx;
        app.run_until_parked();
        assert!(
            app.update(|cx| ws.update(cx, |ws, _| ws.take_toasts()))
                .is_empty()
        );
    }

    /// The branch switched while it waited: it runs for the one checked out then.
    #[gpui_kit::test]
    fn a_waiting_command_reads_the_branch_when_it_runs(cx: &mut TestAppContext) {
        let (ws, cx) = waiting(cx);
        cx.update(|window, cx| {
            let root = window.root::<Root>().flatten().unwrap();
            root.update(cx, |root, cx| {
                root.status = Some(github("next"));
                cx.notify();
            });
        });
        cx.run_until_parked();
        let toasts = toasts(&ws, cx);
        assert_eq!(toasts.len(), 1);
        assert_eq!(
            toasts[0].message.as_ref(),
            "There's no open pull request for next."
        );
    }

    /// A failure whose retry is on its way isn't told: the command waits for the retry.
    #[gpui_kit::test]
    fn an_old_failure_waits_for_its_retry(cx: &mut TestAppContext) {
        setup(cx);
        let next = |cx: &mut TestAppContext| cx.update(|cx| next(Cmd::Open, &on("fix"), cx));
        cx.update(|cx| read_account(Err("offline".into()), cx));
        cx.run_until_parked();
        assert_eq!(next(cx), Next::Failed("offline".into()));
        cx.update(|cx| read_account(account(None), cx));
        assert_eq!(next(cx), Next::Wait);
        cx.run_until_parked();
        // The same for a list.
        cx.update(|cx| {
            for (i, (.., key)) in branch_lists(cx).into_iter().enumerate() {
                let answer: Result<Vec<PullItem>, String> = if i == 0 {
                    Err("offline".to_string())
                } else {
                    Ok(vec![])
                };
                Cache::load(cx, &key, MIN_AGE, move |_| answer.clone());
            }
        });
        cx.run_until_parked();
        assert_eq!(next(cx), Next::Failed("offline".into()));
        cx.update(|cx| read_lists(vec![], cx));
        assert_eq!(next(cx), Next::Wait);
        cx.run_until_parked();
        assert!(matches!(next(cx), Next::NotRun(..)));
    }

    /// A busy repository's first page may leave the branch's PR out: GitHub is asked for it.
    #[gpui_kit::test]
    fn past_a_full_page_github_is_asked_for_the_branchs_pull_request(cx: &mut TestAppContext) {
        setup(cx);
        cx.update(|cx| read_account(account(None), cx));
        cx.run_until_parked();
        let page: Vec<PullItem> = (1..=PAGE as u64)
            .map(|n| PullItem {
                head_ref: format!("b{n}"),
                ..pull(n)
            })
            .collect();
        cx.update(|cx| read_lists(page, cx));
        let head = head_key(None, "o:fix");
        cx.update(|cx| Cache::load(cx, &head, MIN_AGE, |_| Ok(vec![pull(4242)])));
        cx.run_until_parked();
        let next = |cmd, cx: &mut TestAppContext| cx.update(|cx| next(cmd, &on("fix"), cx));
        assert_eq!(next(Cmd::Open, cx), Next::Open(pull(4242)));
        assert_eq!(
            next(Cmd::Create, cx),
            Next::NotRun(
                "#4242 already open for this branch.".into(),
                Some(Fix::Open(Box::new(pull(4242))))
            )
        );
        // A page with room left holds every open one: nothing is asked.
        cx.update(|cx| {
            Cache::refetch_all(cx);
            read_account(account(None), cx);
        });
        cx.run_until_parked();
        cx.update(|cx| read_lists(vec![], cx));
        cx.run_until_parked();
        assert!(matches!(next(Cmd::Open, cx), Next::NotRun(..)));
        assert!(!cx.update(|cx| Cache::read::<Vec<PullItem>>(cx, &head).loading));
    }

    /// New on the fork's own pane makes it the palette's target too.
    #[gpui_kit::test]
    fn create_goes_where_new_last_went(cx: &mut TestAppContext) {
        setup(cx);
        cx.update(|cx| read_account(account(Some(access("up"))), cx));
        cx.run_until_parked();
        cx.update(|cx| read_lists(vec![], cx));
        cx.run_until_parked();
        let target =
            |cx: &mut TestAppContext| match cx.update(|cx| next(Cmd::Create, &on("fix"), cx)) {
                Next::Create { target, .. } => target,
                other => panic!("{other:?}"),
            };
        cx.update(|cx| create_target(&access("o"), Some(&access("up")), Some(false), cx));
        assert_eq!(target(cx), access("o"));
        cx.update(|cx| create_target(&access("o"), Some(&access("up")), Some(true), cx));
        assert_eq!(target(cx), access("up"));
    }
}
