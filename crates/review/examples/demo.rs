//! A guide tab on a repo, for screenshots and memory checks. It never runs a real agent CLI:
//! the Guided Review command is `--agent PATH` (a stand-in that prints a canned guide), or one
//! that doesn't exist.
//!
//! review-demo [REPO] [--agent PATH] [--of changes|commit|branch] [--risks] [--done N]
//!   [--scroll PX] [--section N] [--cycles N]
//!
//! `--risks`: Find Risks once the guide is in. `--done N`: section N (from 1) marked reviewed.
//! `--section N`: the guide and its diffs at section N.
//! `--hidden`: the tab isn't drawn (what drawing costs, over cycles).
//! `--cycles N`: the tab opened, its guide and diffs loaded, and closed N times, with the
//! process's memory printed before, after the first and after the last.

use std::sync::Arc;
use std::time::Duration;

use gitviber_core::api;
use gitviber_review::GuideView;
use gitviber_ui::memory::footprint;
use gitviber_ui::workspace::{GuideOf, Selection, TabGroup, Workspace, selection_key};
use gitviber_ui::{ActiveAppTheme as _, Settings, dev};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Task, Window, div,
};

#[derive(Clone)]
struct Args {
    agent: Option<String>,
    of: String,
    risks: bool,
    done: Option<usize>,
    scroll: f32,
    section: Option<usize>,
    cycles: usize,
    hidden: bool,
}

fn args() -> Args {
    let mut a = Args {
        agent: None,
        of: "changes".into(),
        risks: false,
        done: None,
        scroll: 0.,
        section: None,
        cycles: 0,
        hidden: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--agent" => a.agent = it.next(),
            "--of" => a.of = it.next().unwrap_or_default(),
            "--risks" => a.risks = true,
            "--done" => a.done = it.next().and_then(|n| n.parse().ok()),
            "--scroll" => a.scroll = it.next().and_then(|n| n.parse().ok()).unwrap_or(0.),
            "--hidden" => a.hidden = true,
            "--section" => a.section = it.next().and_then(|n| n.parse().ok()),
            "--cycles" => a.cycles = it.next().and_then(|n| n.parse().ok()).unwrap_or(0),
            _ => {}
        }
    }
    a
}

fn mb() -> String {
    footprint()
        .map(|b| format!("{}MB", b / 1_048_576))
        .unwrap_or_default()
}

struct Demo {
    workspace: Entity<Workspace>,
    /// The tab's view built but not drawn: to tell what drawing costs.
    hidden: bool,
    _task: Task<()>,
    _ws: Subscription,
}

impl Render for Demo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let tab = self
            .workspace
            .read(cx)
            .tabs()
            .active()
            .and_then(|t| t.item.handle().map(|h| h.view()))
            .filter(|_| !self.hidden);
        div().size_full().bg(t.background).children(tab)
    }
}

fn guide_of(kind: &str, cx: &mut gpui_kit::App) -> Option<GuideOf> {
    Some(match kind {
        "commit" => {
            let core = gitviber_ui::core(cx);
            let commit = api::history::log(&core, None, 0, 1, None, None)
                .ok()?
                .into_iter()
                .next()?;
            GuideOf::Commit {
                commit: Arc::new(commit),
            }
        }
        "branch" => GuideOf::Branch {
            base: "refs/heads/main".into(),
            label: "main".into(),
        },
        _ => GuideOf::Changes,
    })
}

fn guide_view(
    ws: &Entity<Workspace>,
    sel: &Selection,
    cx: &gpui_kit::App,
) -> Option<Entity<GuideView>> {
    ws.read(cx)
        .tab_for(sel)
        .and_then(|t| t.item.handle())
        .and_then(|h| h.view().downcast::<GuideView>().ok())
}

fn main() {
    let a = args();
    dev::run(move |window, cx| {
        gitviber_diff::init(cx);
        gitviber_diff::set_core(gitviber_ui::core(cx), cx);
        gitviber_refs::init(cx);
        gitviber_review::init(cx);
        // Never the real CLI: a stand-in, or a command that isn't there.
        let agent = a
            .agent
            .clone()
            .unwrap_or_else(|| "review-demo-no-agent".into());
        Settings::update(cx, |s| {
            s.review_command = Some(agent);
            s.review_language = "English".into();
        });
        let workspace =
            Workspace::for_window(window, cx).expect("dev::run registers the workspace");
        cx.new(|cx: &mut Context<Demo>| {
            let ws = cx.observe(&workspace, |_, _, cx| cx.notify());
            let a = a.clone();
            let task = cx.spawn_in(window, async move |this, cx| {
                let exec = cx.background_executor().clone();
                let wait = |ms| exec.timer(Duration::from_millis(ms));
                wait(800).await;
                let Ok(Some(of)) = cx.update(|_, cx| guide_of(&a.of, cx)) else {
                    eprintln!("demo: nothing to review");
                    return;
                };
                let sel = Selection::Guide { of: of.clone() };
                let open = |this: &gpui_kit::WeakEntity<Demo>,
                            cx: &mut gpui_kit::AsyncWindowContext| {
                    let of = of.clone();
                    let _ = this.update_in(cx, move |_, window, cx| {
                        gitviber_review::open_guide(of, window, cx)
                    });
                };
                let ws = this.update(cx, |d, _| d.workspace.clone()).ok();
                let Some(ws) = ws else { return };
                // The guide and its files in: a guide's tab with its diffs filled.
                let loaded = async |cx: &mut gpui_kit::AsyncWindowContext| {
                    for _ in 0..100 {
                        wait(100).await;
                        let n = cx
                            .update(|_, cx| {
                                guide_view(&ws, &sel, cx)
                                    .map_or(0, |v| v.read(cx).diff_files().len())
                            })
                            .unwrap_or(0);
                        if n > 0 {
                            return n;
                        }
                    }
                    0
                };
                if a.cycles > 0 {
                    eprintln!("demo: before {}", mb());
                    for i in 0..a.cycles {
                        open(&this, cx);
                        let n = loaded(cx).await;
                        if i == 0 {
                            eprintln!("demo: first open, {n} files in its diffs: {}", mb());
                        } else if n == 0 {
                            eprintln!("demo: open {i} showed no diffs");
                        }
                        let _ = cx.update(|_, cx| {
                            ws.update(cx, |ws, cx| ws.close_group(TabGroup::All, cx))
                        });
                        wait(200).await;
                    }
                    wait(500).await;
                    eprintln!("demo: after {} open-close cycles {}", a.cycles, mb());
                    eprintln!("demo: live {:?}", gitviber_ui::memory::live_counts());
                    let _ = cx.update(|_, cx| {
                        eprintln!(
                            "demo: tab still open: {}",
                            ws.read(cx).tab_for(&sel).is_some()
                        )
                    });
                }
                open(&this, cx);
                let n = loaded(cx).await;
                eprintln!("demo: guide open, {n} files in its diffs");
                // Laid out once before it's scrolled, as a click would find it.
                wait(1000).await;
                let _ = cx.update(|_, cx| {
                    let Some(v) = guide_view(&ws, &sel, cx) else {
                        return;
                    };
                    v.update(cx, |v, cx| {
                        if a.risks {
                            v.find_risks(cx);
                        }
                        if let Some(n) = a.done.filter(|n| *n > 0) {
                            v.mark_section(n - 1, true, cx);
                        }
                        if let Some(n) = a.section {
                            v.show_section(n, cx);
                        }
                        if a.scroll > 0. {
                            let s = v.stacked().clone();
                            s.update(cx, |s, cx| s.scroll_by(gpui_kit::px(a.scroll), cx));
                        }
                    });
                });
                eprintln!("demo: {} selection {}", a.of, selection_key(&sel));
            });
            Demo {
                workspace: workspace.clone(),
                hidden: a.hidden,
                _task: task,
                _ws: ws,
            }
        })
        .into()
    });
}
