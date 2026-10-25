//! A guided review's section, risk or whole change handed to the user's agent CLI in the
//! worktree's terminal, for their own questions there: as a paste would put it, in a new Claude
//! Code session's prompt box (core's handoff.rs), or the agent already running there. Another CLI
//! gets it on the clipboard, as it has no way in at start.

use std::path::{Path, PathBuf};

use gitviber_core::api;
use gitviber_core::git::Worktree;
use gitviber_terminal::{TerminalPane, TerminalPanel};
use gitviber_ui::workspace::{Toast, Workspace};
use gpui_kit::{App, AppContext as _, ClipboardItem, Entity, WeakEntity, Window};

use crate::model::agent::{ReviewAgent, effort_of, model_of, preset_of, program_of};
use crate::model::handoff::{
    About, HandoffInput, as_prompt, handoff_context, handoff_name, placed_in, risk_prompt,
};

/// The terminal panel's id, as the terminal crate registers it.
const TERMINAL: &str = "terminal";

fn toast(toast: Toast, window: &mut Window, cx: &mut App) {
    match Workspace::for_window(window, cx) {
        Some(ws) => ws.update(cx, |ws, cx| ws.toast(toast, cx)),
        None => toast.show(window, cx),
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// The window's terminal panel, shown.
fn terminal(window: &mut Window, cx: &mut App) -> Option<Entity<TerminalPanel>> {
    let ws = Workspace::for_window(window, cx)?;
    ws.update(cx, |ws, cx| ws.show_panel(TERMINAL, cx));
    // Built outside the workspace's update: the terminal panel reads the workspace as it's made.
    let panel = Workspace::ensure_panel(&ws, TERMINAL, window, cx)?;
    panel.view().downcast::<TerminalPanel>().ok()
}

/// A new terminal tab in `root`, `run` typed at its first prompt.
fn open_terminal(root: PathBuf, run: Option<String>, window: &mut Window, cx: &mut App) -> bool {
    let Some(panel) = terminal(window, cx) else {
        return false;
    };
    panel.update(cx, |p, cx| p.open_terminal(root, run, window, cx));
    true
}

/// What's asked: a risk's look into it, else the user's question; None leaves it to them.
fn prompt_of(a: &HandoffInput, question: &str) -> Option<String> {
    let q = question.trim();
    match &a.about {
        About::Risk(_) => Some(risk_prompt(a.checked_out).to_string()),
        _ if !q.is_empty() => Some(format!("My question: {q}")),
        _ => None,
    }
}

/// Where the agent works: the worktree that has the change checked out, so it reads the files
/// on disk and can fix them there, else `open` (the one open), reading the change with git.
fn placed(open: &Path, worktrees: &[Worktree], a: HandoffInput) -> (PathBuf, HandoffInput) {
    let (path, a) = placed_in(a, worktrees);
    (path.map_or_else(|| open.to_path_buf(), PathBuf::from), a)
}

/// In a new terminal tab of the change's worktree: Claude Code with the review in its prompt
/// box, sent when there's a question (the user's, or a risk's), or another CLI with it on the
/// clipboard.
/// `model`: the question box's pick for Claude Code; None runs as Settings say.
pub fn ask_agent(
    open: &Path,
    worktrees: &[Worktree],
    a: HandoffInput,
    question: &str,
    model: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let (root, a) = placed(open, worktrees, a);
    if root != open {
        toast(
            Toast::info(
                "Where the change is checked out, so the agent reads its files and can edit them.",
            )
            .title(format!("In worktree {}", folder_name(&root))),
            window,
            cx,
        );
    }
    let prompt = prompt_of(&a, question);
    let text = as_prompt(&handoff_context(&a), prompt.as_deref());
    let name = handoff_name(&a.of, &a.about);
    start_agent_with(root, text, name, prompt.is_some(), model, window, cx);
}

/// The review agent (Settings) in a new terminal tab of `root`: Claude Code with `text` in its
/// prompt box, another CLI with it on the clipboard. `model`: Claude Code's for this one (""
/// the CLI's own), else the review agent's.
pub fn start_agent_with(
    root: PathBuf,
    text: String,
    name: String,
    send: bool,
    model: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let agent = ReviewAgent::from_settings(gitviber_ui::Settings::get(cx));
    let program = program_of(&agent.command);
    let preset = preset_of(&agent.command);
    if !program.eq_ignore_ascii_case("claude") {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        // A preset's own session; llm and a custom command have none known.
        let session = preset.is_some_and(|p| p.session);
        let detail = if session {
            format!("Paste it into {program}, starting in the terminal.")
        } else {
            "Paste it into your agent.".to_string()
        };
        toast(
            Toast::info(detail).title("Review copied for the agent"),
            window,
            cx,
        );
        if session {
            open_terminal(root, Some(program), window, cx);
        }
        return;
    }
    let (model, effort) = handoff_model(preset, &agent, model);
    let core = gitviber_ui::core(cx);
    let command = agent.command.clone();
    window
        .spawn(cx, async move |cx| {
            let line = cx
                .background_spawn(async move {
                    api::changes::handoff_command(&core, command, model, effort, name, text, send)
                })
                .await;
            let _ = cx.update(|window, cx| match line {
                Ok(line) => {
                    if !open_terminal(root, Some(line), window, cx) {
                        toast(
                            Toast::error("This window has no terminal.")
                                .title("Couldn't hand the review to Claude Code"),
                            window,
                            cx,
                        );
                    }
                }
                Err(e) => toast(
                    Toast::error(e).title("Couldn't hand the review to Claude Code"),
                    window,
                    cx,
                ),
            });
        })
        .detach();
}

/// The model and effort a session starts with: the box's pick, or a preset's as the review ran
/// with; a custom command's are its own. Settings' effort is for Settings' model: another may not
/// take it (haiku has no max).
fn handoff_model(
    preset: Option<&'static crate::model::agent::Preset>,
    agent: &ReviewAgent,
    picked: Option<String>,
) -> (Option<String>, Option<String>) {
    let settings = preset.map(|p| model_of(p, &agent.models));
    let same = picked.is_none() || picked == settings;
    let model = picked.or(settings).filter(|m| !m.is_empty());
    let effort = preset
        .filter(|_| same)
        .map(|p| effort_of(p, &agent.efforts))
        .filter(|e| !e.is_empty());
    (model, effort)
}

/// Into the agent already running in the change's worktree, Enter left to the user: its session
/// is warm. Not while it works or asks something: the paste would land in its dialog or the
/// user's draft.
pub fn paste_to_running_agent(
    open: &Path,
    worktrees: &[Worktree],
    a: HandoffInput,
    window: &mut Window,
    cx: &mut App,
) {
    let (root, a) = placed(open, worktrees, a);
    let text = as_prompt(&handoff_context(&a), prompt_of(&a, "").as_deref());
    let name = folder_name(&root);
    let Some(panel) = terminal(window, cx) else {
        return toast(
            Toast::info("Ask Agent starts one in a new terminal tab.")
                .title(format!("No agent runs in {name}'s terminal")),
            window,
            cx,
        );
    };
    let panes = panes_in(&panel, &root, cx);
    let ids: Vec<u32> = panes.iter().map(|(id, _)| *id).collect();
    let core = gitviber_ui::core(cx);
    window
        .spawn(cx, async move |cx| {
            let agents = cx
                .background_spawn(
                    async move { api::app::pty_agents(&core, ids).unwrap_or_default() },
                )
                .await;
            let _ = cx.update(|window, cx| {
                let found = panes
                    .iter()
                    .find_map(|(id, pane)| Some((agents.get(id)?, pane.upgrade()?)));
                let Some((agent, pane)) = found else {
                    return toast(
                        Toast::info("Ask Agent starts one in a new terminal tab.")
                            .title(format!("No agent runs in {name}'s terminal")),
                        window,
                        cx,
                    );
                };
                match agent.state {
                    Some(state @ ("working" | "waiting")) => {
                        let doing = if state == "working" {
                            "working"
                        } else {
                            "waiting on you"
                        };
                        toast(
                            Toast::info(
                                "Paste once it's done, or ask in a new session with Ask Agent.",
                            )
                            .title(format!("{} is {doing}", agent.name)),
                            window,
                            cx,
                        );
                    }
                    _ => {
                        pane.update(cx, |p, cx| {
                            p.paste_text(text, window, cx);
                            p.focus(window, cx);
                        });
                    }
                }
            });
        })
        .detach();
}

/// `root`'s terminal panes, the one in front first, with their pty ids.
fn panes_in(
    panel: &Entity<TerminalPanel>,
    root: &Path,
    cx: &App,
) -> Vec<(u32, WeakEntity<TerminalPane>)> {
    let front = panel.read(cx).active_pane().map(|p| p.entity_id());
    let mut panes: Vec<(u32, WeakEntity<TerminalPane>)> = panel
        .read(cx)
        .panes()
        .filter(|p| {
            let p = p.read(cx);
            !p.has_exited() && p.cwd() == root
        })
        .filter_map(|p| Some((p.read(cx).pty_id()?, p.downgrade())))
        .collect();
    panes.sort_by_key(|(_, p)| Some(p.entity_id()) != front);
    panes
}

/// `text` pasted into `root`'s terminal, Enter left to the user: into the pane its agent runs
/// in, else the one in front of the worktree's. False when it has none.
pub fn paste_to_terminal(root: &Path, text: String, window: &mut Window, cx: &mut App) -> bool {
    let Some(panel) = terminal(window, cx) else {
        return false;
    };
    let panes = panes_in(&panel, root, cx);
    if panes.is_empty() {
        return false;
    }
    let ids: Vec<u32> = panes.iter().map(|(id, _)| *id).collect();
    let core = gitviber_ui::core(cx);
    window
        .spawn(cx, async move |cx| {
            let agents = cx
                .background_spawn(
                    async move { api::app::pty_agents(&core, ids).unwrap_or_default() },
                )
                .await;
            let _ = cx.update(|window, cx| {
                // A shell would run what's pasted line by line as soon as Enter is pressed.
                let pane =
                    pick_pane(&panes, |id| agents.contains_key(&id)).and_then(|p| p.upgrade());
                if let Some(pane) = pane {
                    pane.update(cx, |p, cx| {
                        p.paste_text(text, window, cx);
                        p.focus(window, cx);
                    });
                }
            });
        })
        .detach();
    true
}

/// The pane with an agent in it, else the first (the one in front).
fn pick_pane<T>(panes: &[(u32, T)], has_agent: impl Fn(u32) -> bool) -> Option<&T> {
    panes
        .iter()
        .find(|(id, _)| has_agent(*id))
        .or(panes.first())
        .map(|(_, p)| p)
}

/// The part asked about, as the agent would get it, on the clipboard.
pub fn copy_as_prompt(a: &HandoffInput, window: &mut Window, cx: &mut App) {
    let text = as_prompt(&handoff_context(a), prompt_of(a, "").as_deref());
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    toast(
        Toast::success("Paste it into the agent's terminal.").title("Review copied for the agent"),
        window,
        cx,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_boxs_model_drops_settings_effort_for_another_model() {
        let mut s = gitviber_ui::Settings {
            review_command: Some("claude -p".into()),
            ..Default::default()
        };
        s.review_efforts.insert("claude".into(), "max".into());
        let agent = ReviewAgent::from_settings(&s);
        let claude = preset_of("claude -p");
        let both = (Some("sonnet".to_string()), Some("max".to_string()));
        assert_eq!(handoff_model(claude, &agent, None), both);
        assert_eq!(handoff_model(claude, &agent, Some("sonnet".into())), both);
        assert_eq!(
            handoff_model(claude, &agent, Some("haiku".into())),
            (Some("haiku".into()), None)
        );
        assert_eq!(
            handoff_model(claude, &agent, Some(String::new())),
            (None, None)
        );
        assert_eq!(handoff_model(None, &agent, None), (None, None));
    }

    #[test]
    fn notes_go_to_the_agents_pane_before_a_shell_in_front() {
        let panes = [(1, "zsh in front"), (2, "claude"), (3, "another zsh")];
        assert_eq!(pick_pane(&panes, |id| id == 2), Some(&"claude"));
        assert_eq!(pick_pane(&panes, |_| false), Some(&"zsh in front"));
        assert_eq!(pick_pane::<&str>(&[], |_| true), None);
    }
}
