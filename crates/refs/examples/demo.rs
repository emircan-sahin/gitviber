//! A window with the top bar's chips and actions in a row, and the branch and worktree pickers
//! open below them, on a repo: for screenshots and memory checks.
//!
//! demo [DIR] [--bar] [--cycles N]
//!
//! `--bar`: only the bar row. `--cycles N` opens and closes both pickers N times first and prints
//! the process's memory before and after. GITVIBER_BACKGROUND=1 opens the window behind the
//! others without taking the keyboard.

use std::time::Duration;

use gitviber_refs::RepoState;
use gitviber_refs::branches::BranchPicker;
use gitviber_refs::worktrees::WorktreePicker;
use gitviber_ui::workspace::{BarSlot, Registry, Workspace};
use gitviber_ui::{ActiveAppTheme, dev};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Task, WeakEntity, Window, div, px,
};

struct Demo {
    workspace: WeakEntity<Workspace>,
    pickers: Option<(Entity<BranchPicker>, Entity<WorktreePicker>)>,
    bar_only: bool,
    /// Read before the first frame: a window behind the others draws only that one.
    load_now: bool,
    _cycle: Task<()>,
    _subs: Vec<Subscription>,
}

fn footprint() -> String {
    let phys = gitviber_ui::memory::footprint().unwrap_or(0);
    format!(
        "phys_footprint={:.1}MB live={:?}",
        phys as f64 / 1_048_576.,
        gitviber_ui::memory::live_counts()
    )
}

impl Demo {
    fn state(&self, cx: &mut Context<Self>) -> Option<Entity<RepoState>> {
        let project = self.workspace.upgrade()?.read(cx).project()?.clone();
        Some(RepoState::get(&project, cx))
    }

    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.state(cx) else { return };

        let b = cx.new(|cx| BranchPicker::new(state.clone(), None, window, cx));
        let w = cx.new(|cx| WorktreePicker::new(state, None, window, cx));
        if self.load_now {
            b.update(cx, |p, cx| p.load_now(cx));
            w.update(cx, |p, cx| p.load_now(cx));
        }
        self.pickers = Some((b, w));
        cx.notify();
    }
}

impl Render for Demo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let ctx = match self.workspace.upgrade() {
            Some(ws) => ws.update(cx, |ws, cx| ws.context(cx)),
            None => return div(),
        };
        let chips: Vec<AnyElement> = Registry::get(cx)
            .bar_items(BarSlot::TopChips)
            .into_iter()
            .filter_map(|i| (i.render)(&ctx, window, cx))
            .collect();
        let actions: Vec<AnyElement> = Registry::get(cx)
            .bar_items(BarSlot::TopActions)
            .into_iter()
            .filter_map(|i| (i.render)(&ctx, window, cx))
            .collect();
        let panel = |child: AnyElement| {
            div()
                .flex_none()
                .border_1()
                .border_color(t.border)
                .rounded(px(8.))
                .bg(t.elevated)
                .shadow_md()
                .child(child)
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.background)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(px(40.))
                    .px_3()
                    .bg(t.chrome(t.sidebar))
                    .border_b_1()
                    .border_color(t.border)
                    .children(chips)
                    .child(div().flex_1())
                    .children(actions),
            )
            .when_pickers(!self.bar_only, self.pickers.clone(), |d, (b, w)| {
                d.child(
                    div()
                        .flex()
                        .gap_4()
                        .p_4()
                        .items_start()
                        .child(panel(b.into_any_element()))
                        .child(panel(w.into_any_element())),
                )
            })
    }
}

trait WhenPickers: Sized {
    fn when_pickers<T>(self, on: bool, v: Option<T>, f: impl FnOnce(Self, T) -> Self) -> Self {
        match v.filter(|_| on) {
            Some(v) => f(self, v),
            None => self,
        }
    }
}
impl WhenPickers for gpui_kit::Div {}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bar_only = args.iter().any(|a| a == "--bar");
    let dialog = args
        .iter()
        .position(|a| a == "--dialog")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let cycles: usize = args
        .iter()
        .position(|a| a == "--cycles")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    dev::run(move |window, cx| {
        gitviber_refs::init(cx);
        let ws = Workspace::for_window(window, cx).expect("dev::run makes the window's workspace");
        let workspace = ws.downgrade();
        // The project opens core's repo in the background; here it's opened now, so the first
        // frame has the repo's state in it.
        let root = dev::project_arg();
        let _ = gitviber_core::api::repo::open_repo(
            &gitviber_ui::core(cx),
            root.to_string_lossy().into_owned(),
        );
        let project = ws.read(cx).project().cloned();
        if let Some(p) = &project {
            RepoState::get(p, cx).update(cx, |s, cx| s.load_now(cx));
        }
        let first = cx.new(|cx: &mut Context<Demo>| {
            let subs = vec![];
            let cycle = if cycles == 0 {
                Task::ready(())
            } else {
                cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(800))
                        .await;
                    eprintln!("demo: before {}", footprint());
                    for i in 0..cycles {
                        let _ = this.update_in(cx, |this, window, cx| this.open(window, cx));
                        cx.background_executor()
                            .timer(Duration::from_millis(500))
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            this.pickers = None;
                            cx.notify();
                        });
                        cx.background_executor()
                            .timer(Duration::from_millis(200))
                            .await;
                        if i == 0 {
                            eprintln!("demo: after one cycle {}", footprint());
                        }
                    }
                    eprintln!("demo: after {cycles} open-close cycles {}", footprint());
                    let _ = this.update_in(cx, |this, window, cx| this.open(window, cx));
                })
            };
            Demo {
                workspace,
                pickers: None,
                bar_only,
                load_now: cycles == 0,
                _cycle: cycle,
                _subs: subs,
            }
        });
        if cycles == 0 {
            first.update(cx, |d, cx| d.open(window, cx));
        }
        // A dialog, opened before the first frame: `--dialog new-worktree|new-branch|rename|lock`.
        if let Some(which) = dialog.clone() {
            let handle = window.window_handle();
            let state = project.as_ref().map(|p| RepoState::get(p, cx));
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let Some(state) = state else { return };
                    let branch = state
                        .read(cx)
                        .branches
                        .iter()
                        .find(|b| b.current)
                        .cloned()
                        .unwrap_or_default();
                    let wt = gitviber_refs::worktrees::list(&state.read(cx).worktrees)
                        .into_iter()
                        .find(|w| w.current)
                        .unwrap_or_default();
                    match which.as_str() {
                        "new-branch" => gitviber_refs::branches::open_new_branch(
                            format!("refs/heads/{}", branch.name),
                            window,
                            cx,
                        ),
                        "rename" => gitviber_refs::branches::open_rename_branch(branch, window, cx),
                        "lock" => gitviber_refs::worktrees::open_lock(wt, window, cx),
                        _ => gitviber_refs::worktrees::open_new_worktree(None, window, cx),
                    }
                });
            });
        }
        first.into()
    });
}
