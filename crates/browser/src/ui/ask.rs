//! A tab's picks or errors handed to the user's agent CLI in a new terminal of its worktree:
//! Claude Code with them in its prompt box (sent when there's a question), another CLI with
//! them on the clipboard, as the guided review's Ask Agent does.

use gitviber_core::api;
use gitviber_terminal::{TerminalPane, TerminalPanel};
use gitviber_ui::settings::AgentPreset;
use gitviber_ui::workspace::{Toast, Workspace};
use gpui::{App, AppContext as _, ClipboardItem, Entity, Window};
use std::collections::BTreeMap;
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
    let panel = if show {
        ws.update(cx, |ws, cx| ws.show_panel(TERMINAL, cx));
        // Built outside the workspace's update: the panel reads the workspace as it's made.
        Workspace::ensure_panel(&ws, TERMINAL, window, cx)
    } else {
        ws.read(cx).built_panel(TERMINAL).cloned()
    }?;
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

/// A line the new terminal runs at its first prompt: typed alone, it waited there for an Enter.
fn run(line: &str) -> String {
    format!("{line}\r")
}

/// Claude Code's own names for its newest models of each size; Settings takes any id.
const CLAUDE_MODELS: [&str; 3] = ["opus", "sonnet", "haiku"];

/// The agent CLI Ask Agent runs (Settings → Guided Review's, else the suggest command), its
/// preset if it's one, and the models and efforts typed per preset.
struct Agent {
    command: String,
    preset: Option<&'static AgentPreset>,
    models: BTreeMap<String, String>,
    efforts: BTreeMap<String, String>,
}

fn agent(cx: &App) -> Agent {
    let s = gitviber_ui::Settings::get(cx);
    let command = s
        .review_command
        .clone()
        .unwrap_or_else(|| s.suggest_command.clone());
    let mut models = s.suggest_models.clone();
    models.extend(s.review_models.clone());
    let mut efforts = s.suggest_efforts.clone();
    efforts.extend(s.review_efforts.clone());
    Agent {
        preset: AgentPreset::of(&command),
        command,
        models,
        efforts,
    }
}

fn program_of(command: &str) -> String {
    command
        .split_whitespace()
        .next()
        .and_then(|w| w.rsplit('/').next())
        .unwrap_or_default()
        .to_string()
}

/// The agent's name for the question's label.
pub fn agent_name(cx: &App) -> String {
    let program = program_of(&agent(cx).command);
    if program.eq_ignore_ascii_case("claude") {
        "Claude Code".into()
    } else if program.is_empty() {
        "the agent".into()
    } else {
        program
    }
}

/// The models the question's box offers and the one it starts on: the last picked, else
/// Settings'. Only for Claude Code, the one agent that starts with the question in hand.
pub fn model_choices(last: Option<&str>, cx: &App) -> Option<(Vec<String>, String)> {
    let a = agent(cx);
    let preset = a.preset.filter(|p| p.key == "claude")?;
    let settings = preset.model_in(&a.models);
    let value = last.map_or_else(|| settings.clone(), str::to_string);
    let mut choices = vec![value.clone(), settings];
    choices.extend(CLAUDE_MODELS.map(String::from));
    let mut seen = std::collections::HashSet::new();
    choices.retain(|c| seen.insert(c.clone()));
    Some((choices, value))
}

/// What goes into a terminal: no control characters, no trailing blank lines.
fn for_terminal(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let clean: String = text
        .chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .collect();
    clean.trim_end_matches('\n').to_string()
}

/// What's asked, with what it's about, as one prompt; and whether it's sent at once.
fn prompt(content: &str, question: &str) -> (String, bool) {
    let q = question.trim();
    let ask = if q.is_empty() {
        "My question follows. If none does, just say in one line that you're ready, and wait for it."
            .to_string()
    } else {
        format!("My question: {q}")
    };
    (format!("{}\n\n{ask}", for_terminal(content)), !q.is_empty())
}

/// `content` to the agent in a new terminal tab of `root`, named `name`: Claude Code with it in
/// its prompt box (sent with a question), started on `model` (None: Settings'); another CLI with
/// it on the clipboard, its own session started when it has one.
pub fn ask_agent(
    root: &str,
    content: &str,
    name: String,
    question: &str,
    model: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let a = agent(cx);
    let (text, send) = prompt(content, question);
    let root = PathBuf::from(root);
    if !program_of(&a.command).eq_ignore_ascii_case("claude") {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        let session = a
            .preset
            .filter(|p| p.session)
            .map(|_| program_of(&a.command));
        let detail = match &session {
            Some(program) => format!("Paste it into {program}, starting in the terminal."),
            None => "Paste it into your agent.".into(),
        };
        toast(
            Toast::info(detail).title("Copied for the agent"),
            window,
            cx,
        );
        if let (Some(program), Some(panel)) = (session, terminal(window, cx, true)) {
            panel.update(cx, |p, cx| {
                p.open_terminal(root, Some(run(&program)), window, cx)
            });
        }
        return;
    }
    let settings_model = a.preset.map(|p| p.model_in(&a.models));
    // Settings' effort is for Settings' model: another may not take it (haiku has no max).
    let effort = a
        .preset
        .filter(|_| model.is_none() || model == settings_model)
        .map(|p| p.effort_in(&a.efforts))
        .filter(|e| !e.is_empty());
    let model = model.or(settings_model).filter(|m| !m.is_empty());
    let core = gitviber_ui::core(cx);
    let command = a.command;
    window
        .spawn(cx, async move |cx| {
            let line = cx
                .background_spawn(async move {
                    api::changes::handoff_command(&core, command, model, effort, name, text, send)
                })
                .await;
            let _ = cx.update(|window, cx| match line {
                Ok(line) => match terminal(window, cx, true) {
                    Some(panel) => panel.update(cx, |p, cx| {
                        p.open_terminal(root, Some(run(&line)), window, cx)
                    }),
                    None => toast(
                        Toast::error("This window has no terminal.")
                            .title("Couldn't hand it to Claude Code"),
                        window,
                        cx,
                    ),
                },
                Err(e) => toast(
                    Toast::error(e).title("Couldn't hand it to Claude Code"),
                    window,
                    cx,
                ),
            });
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{div, Context, IntoElement, Render, TestAppContext};

    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// Ask before any terminal was opened builds the panel; built inside the workspace's update,
    /// its constructor reading the workspace panicked, which aborts the app in a key handler.
    #[gpui::test]
    fn ask_builds_the_terminal_panel_when_none_is_yet(cx: &mut TestAppContext) {
        let base = gitviber_ui::test_support::temp_dir("browser-ask");
        std::fs::create_dir_all(base.join("home")).unwrap();
        // SAFETY: nextest gives each test its own process; nothing else reads these meanwhile.
        unsafe {
            std::env::set_var("GITVIBER_CONFIG_DIR", base.join("config"));
            std::env::set_var("HOME", base.join("home"));
        }
        cx.update(|cx| {
            gitviber_ui::init(cx);
            gitviber_ui::install_core(cx);
            gitviber_terminal::init(cx);
        });
        let (_, cx) = cx.add_window_view(|_, _| Host);
        let ws = cx.update(|window, cx| {
            let ws = cx.new(Workspace::new);
            Workspace::register_window(&ws, window, cx);
            ws
        });
        let built = |cx: &mut gpui::VisualTestContext| {
            ws.read_with(cx, |ws, _| ws.built_panel(TERMINAL).is_some())
        };
        assert!(!built(cx));
        assert!(cx
            .update(|window, cx| terminal(window, cx, false))
            .is_none());
        let panel = cx
            .update(|window, cx| terminal(window, cx, true))
            .expect("the terminal panel");
        assert!(built(cx));
        cx.run_until_parked();
        panel.update(cx, |p, cx| p.close_all(cx));
        cx.update(|window, _| window.remove_window());
        cx.run_until_parked();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_handed_line_runs_at_the_prompt() {
        assert_eq!(run("sh '/tmp/start.sh'"), "sh '/tmp/start.sh'\r");
    }

    #[test]
    fn a_prompt_is_sent_only_with_a_question_and_carries_no_control_characters() {
        let (text, send) = prompt("a\x1b[31m\r\nb\n\n", "  why?  ");
        assert_eq!(text, "a[31m\nb\n\nMy question: why?");
        assert!(send);
        let (text, send) = prompt("x", " ");
        assert!(!send);
        assert!(text.ends_with("and wait for it."));
    }
}
