//! A tab's picks or errors handed to the user's agent CLI in a new terminal of its worktree:
//! Claude Code with them in its prompt box (sent when there's a question), another CLI with
//! them on the clipboard, as the guided review's Ask Agent does.

use gitviber_core::api;
use gitviber_terminal::{TerminalPane, TerminalPanel};
use gitviber_ui::workspace::{Toast, Workspace};
use gpui::{App, AppContext as _, ClipboardItem, Entity, Window};
use std::path::PathBuf;

/// The terminal panel's id, as the terminal crate registers it.
const TERMINAL: &str = "terminal";

fn toast(toast: Toast, window: &mut Window, cx: &mut App) {
    match Workspace::for_window(window, cx) {
        Some(ws) => ws.update(cx, |ws, cx| ws.toast(toast, cx)),
        None => toast.show(window, cx),
    }
}

/// The window's terminal panel, when it has one built.
pub fn terminal(window: &mut Window, cx: &mut App, show: bool) -> Option<Entity<TerminalPanel>> {
    let ws = Workspace::for_window(window, cx)?;
    let panel = ws.update(cx, |ws, cx| {
        if show {
            ws.show_panel(TERMINAL, cx);
            ws.panel(TERMINAL, window, cx)
        } else {
            ws.built_panel(TERMINAL).cloned()
        }
    })?;
    panel.view().downcast::<TerminalPanel>().ok()
}

/// The terminals of the window open in `root`, by their pty: what the ports menu reads.
pub fn ptys_in(root: &str, window: &mut Window, cx: &mut App) -> Vec<u32> {
    let Some(panel) = terminal(window, cx, false) else {
        return Vec::new();
    };
    let panes: Vec<Entity<TerminalPane>> = panel.read(cx).panes().cloned().collect();
    panes
        .iter()
        .filter(|p| p.read(cx).cwd().starts_with(root))
        .filter_map(|p| p.read(cx).pty_id())
        .collect()
}

/// What's asked, with what it's about, as one prompt.
fn prompt(content: &str, question: &str) -> (String, bool) {
    let q = question.trim();
    if q.is_empty() {
        (content.to_string(), false)
    } else {
        (format!("{content}\n\nMy question: {q}"), true)
    }
}

/// `content` to the agent in a new terminal tab of `root`, named `name`.
pub fn ask_agent(
    root: &str,
    content: &str,
    name: String,
    question: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let settings = gitviber_ui::Settings::get(cx);
    let command = settings
        .review_command
        .clone()
        .unwrap_or_else(|| settings.suggest_command.clone());
    let program = command
        .split_whitespace()
        .next()
        .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
        .unwrap_or_default();
    let (text, send) = prompt(content, question);
    let root = PathBuf::from(root);
    if !program.eq_ignore_ascii_case("claude") {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        toast(
            Toast::info("Paste it into your agent.").title("Copied for the agent"),
            window,
            cx,
        );
        return;
    }
    let core = gitviber_ui::core(cx);
    window
        .spawn(cx, async move |cx| {
            let line = cx
                .background_spawn(async move {
                    api::changes::handoff_command(&core, command, None, None, name, text, send)
                })
                .await;
            let _ = cx.update(|window, cx| match line {
                Ok(line) => match terminal(window, cx, true) {
                    Some(panel) => {
                        panel.update(cx, |p, cx| p.open_terminal(root, Some(line), window, cx))
                    }
                    None => toast(
                        Toast::error("This window has no terminal.")
                            .title("Couldn't ask the agent"),
                        window,
                        cx,
                    ),
                },
                Err(e) => toast(Toast::error(e).title("Couldn't ask the agent"), window, cx),
            });
        })
        .detach();
}
