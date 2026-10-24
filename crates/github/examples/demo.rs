//! The PRs or Issues panel beside the tab it opens, on a repository, for pictures and memory
//! checks. Reads GitHub only; nothing here writes to it.
//!
//! github-demo [--issues] [--pull N] [--issue N] [--file N] [--cycles N --pulls a,b,c] [REPO]
//!
//! `--file N` opens PR #N's first changed file. `--cycles N` opens the `--pulls` tabs, lets them
//! load, closes them, N times, and prints the memory footprint after each step.
//! GITVIBER_BACKGROUND=1 opens behind the others; GITVIBER_SNAPSHOT=out.png draws off screen.

use std::time::Duration;

use gitviber_core::api;
use gitviber_github::model::{IssueItem, PullItem, PullState};
use gitviber_ui::dev;
use gitviber_ui::memory::{footprint, format_bytes, live_counts};
use gitviber_ui::workspace::{PullRange, TabGroup, Workspace};
use gpui_kit::{
    AnyView, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Task, Window, div, px,
};

#[derive(Clone, Default)]
struct Args {
    issues: bool,
    pull: Option<u64>,
    issue: Option<u64>,
    file: Option<u64>,
    cycles: usize,
    pulls: Vec<u64>,
}

fn args() -> Args {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    let num = |s: Option<String>| s.and_then(|s| s.parse().ok());
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--issues" => a.issues = true,
            "--pull" => a.pull = num(it.next()),
            "--issue" => a.issue = num(it.next()),
            "--file" => a.file = num(it.next()),
            "--cycles" => a.cycles = num(it.next()).unwrap_or(0) as usize,
            "--pulls" => {
                a.pulls = it
                    .next()
                    .unwrap_or_default()
                    .split(',')
                    .filter_map(|n| n.parse().ok())
                    .collect()
            }
            _ => {}
        }
    }
    a
}

/// owner/name of the repository's origin on GitHub.
fn origin_repo() -> String {
    gitviber_core::github::origin_repo(&dev::project_arg())
        .unwrap_or_else(|| "emircan-sahin/gitviber".into())
}

fn pull(n: u64) -> PullItem {
    PullItem {
        number: n,
        title: String::new(),
        state: PullState::Open,
        draft: false,
        author: String::new(),
        head_ref: String::new(),
        head_sha: String::new(),
        head_repo: None,
        base_ref: String::new(),
        base_sha: String::new(),
        created_at: String::new(),
        updated_at: String::new(),
        url: format!("https://github.com/{}/pull/{n}", origin_repo()),
    }
}

fn issue(n: u64) -> IssueItem {
    IssueItem {
        number: n,
        title: String::new(),
        open: true,
        state_reason: None,
        author: String::new(),
        labels: vec![],
        assignees: vec![],
        comments: 0,
        created_at: String::new(),
        updated_at: String::new(),
        url: format!("https://github.com/{}/issues/{n}", origin_repo()),
    }
}

fn memory(step: &str) {
    let ours: Vec<String> = live_counts()
        .into_iter()
        .filter(|(k, _)| {
            [
                "PullView",
                "IssueView",
                "PullFileView",
                "DiffView",
                "PullsPanel",
            ]
            .contains(k)
        })
        .map(|(k, n)| format!("{k}={n}"))
        .collect();
    eprintln!(
        "demo: {step}: footprint {} live [{}]",
        footprint().map(format_bytes).unwrap_or_default(),
        ours.join(" ")
    );
}

struct Demo {
    ws: Entity<Workspace>,
    issues: bool,
    _work: Task<()>,
}

impl Demo {
    fn new(ws: Entity<Workspace>, a: Args, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe(&ws, |_, _, cx| cx.notify()).detach();
        let weak = ws.downgrade();
        if let Some(n) = a.pull {
            gitviber_github::open_pull(&weak, pull(n), false, window, cx);
        }
        if let Some(n) = a.issue {
            gitviber_github::open_issue(&weak, issue(n), false, window, cx);
        }
        let work = if let Some(n) = a.file {
            Self::open_file(ws.downgrade(), n, window, cx)
        } else if a.cycles > 0 {
            Self::cycles(ws.downgrade(), a.clone(), window, cx)
        } else {
            Task::ready(())
        };
        Demo {
            ws,
            issues: a.issues,
            _work: work,
        }
    }

    /// PR #n's first file, as its page opens it.
    fn open_file(
        ws: gpui_kit::WeakEntity<Workspace>,
        n: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let core = gitviber_ui::core(cx);
        cx.spawn_in(window, async move |_, cx| {
            let target = Some(origin_repo());
            let read = cx
                .background_spawn(async move {
                    let d = api::github::pr_detail(&core, target.clone(), n)?;
                    let f = api::github::pr_files(
                        &core,
                        target,
                        n,
                        d.pull.base_ref.clone(),
                        d.pull.base_sha.clone(),
                        d.pull.head_sha.clone(),
                        "demo".into(),
                        |_| {},
                    )?;
                    Ok::<_, String>((d.pull.url, f))
                })
                .await;
            let Ok((url, files)) = read.inspect_err(|e| eprintln!("demo: {e}")) else {
                return;
            };
            let Some(file) = files.files.first().cloned() else {
                return;
            };
            let range = PullRange {
                number: Some(n),
                pull_url: Some(url),
                label: None,
                base: files.base.clone(),
                head: files.head.clone(),
            };
            cx.update(|window, cx| {
                gitviber_github::open_pull_file(&ws, range, file, false, window, cx);
            })
            .ok();
        })
    }

    fn cycles(
        ws: gpui_kit::WeakEntity<Workspace>,
        a: Args,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn_in(window, async move |_, cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            memory("before");
            for round in 1..=a.cycles {
                for n in &a.pulls {
                    let ws = ws.clone();
                    cx.update(|window, cx| {
                        gitviber_github::open_pull(&ws, pull(*n), false, window, cx)
                    })
                    .ok();
                    cx.background_executor()
                        .timer(Duration::from_millis(1500))
                        .await;
                }
                cx.background_executor().timer(Duration::from_secs(3)).await;
                memory(&format!("round {round}: {} tabs open", a.pulls.len()));
                let ws2 = ws.clone();
                cx.update(|_, cx| {
                    ws2.update(cx, |ws, cx| ws.close_group(TabGroup::All, cx))
                        .ok()
                })
                .ok();
                cx.background_executor().timer(Duration::from_secs(3)).await;
                memory(&format!("round {round}: closed"));
            }
            eprintln!("demo: done");
        })
    }
}

impl Render for Demo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = if self.issues { "issues" } else { "pulls" };
        let panel = self
            .ws
            .update(cx, |ws, cx| ws.panel(id, window, cx))
            .map(|p| p.view());
        let tab: Option<AnyView> = self.ws.update(cx, |ws, cx| ws.active_view(window, cx));
        let t = gitviber_ui::ActiveAppTheme::app_theme(&**cx).clone();
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .w(px(380.))
                    .flex_none()
                    .h_full()
                    .border_r_1()
                    .border_color(t.border)
                    .bg(t.sidebar)
                    .children(panel),
            )
            .child(div().flex_1().min_w_0().h_full().children(tab))
    }
}

fn main() {
    let a = args();
    dev::run(move |window, cx| {
        let core = gitviber_ui::core(cx);
        gitviber_diff::set_core(core, cx);
        gitviber_diff::init(cx);
        gitviber_github::init(cx);
        let ws = Workspace::for_window(window, cx).expect("dev::run registers its workspace");
        cx.new(|cx| Demo::new(ws, a, window, cx)).into()
    });
}
