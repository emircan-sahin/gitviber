//! Resume a Conversation…: an agent's past conversations in a folder (core's conversations.rs,
//! read each time it's asked: one started a minute ago is the likely pick), the picked one resumed
//! in a new terminal there, or shown where a terminal already runs it.

use gitviber_core::conversations::Conversation;
use gitviber_ui::memory::Live;
use gpui::{FontWeight, Render, SharedString, rems};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::command::{Command, CommandItem, CommandState};

use super::*;

pub(super) struct ConversationPicker {
    state: Entity<CommandState>,
    panel: WeakEntity<TerminalPanel>,
    cwd: PathBuf,
    /// None while they're read.
    list: Option<Result<Vec<Conversation>, String>>,
    _read: Task<()>,
    _live: Live,
}

impl TerminalPanel {
    /// The picker for `cwd`'s conversations, over the window.
    pub(super) fn ask_conversation(
        &mut self,
        cwd: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let (core, panel, dir) = (
            self.core.clone(),
            cx.weak_entity(),
            cwd.to_string_lossy().into_owned(),
        );
        let picker = cx.new(|cx| {
            let read = cx.spawn(async move |this: WeakEntity<ConversationPicker>, cx| {
                let list = cx
                    .background_spawn(async move {
                        gitviber_core::api::app::agent_conversations(&core, dir)
                    })
                    .await;
                let _ = this.update(cx, |p, cx| {
                    p.list = Some(list);
                    cx.notify();
                });
            });
            ConversationPicker {
                state: cx.new(|cx| CommandState::new(window, cx)),
                panel,
                cwd,
                list: None,
                _read: read,
                _live: Live::new("ConversationPicker"),
            }
        });
        let first = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                let state = picker.read(cx).state.clone();
                state.update(cx, |s, cx| s.focus(window, cx));
            }
            dialog
                .w(px(576.))
                .margin_top(px(90.))
                .close_button(false)
                .p_0()
                .child(picker.clone())
        });
    }

    /// The pane whose agent is in conversation `id` now: resumed again, it would run twice.
    fn pane_of_conversation(&self, id: &str) -> Option<PaneId> {
        self.panes
            .iter()
            .find(|(_, s)| s.agent.as_ref().and_then(|a| a.session.as_deref()) == Some(id))
            .map(|(id, _)| *id)
    }
}

impl ConversationPicker {
    fn confirm(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(list)) = &self.list else { return };
        let Some(c) = list.get(row) else { return };
        let (id, command, cwd, panel) = (
            c.id.clone(),
            c.command.clone(),
            self.cwd.clone(),
            self.panel.clone(),
        );
        window.close_dialog(cx);
        window.defer(cx, move |window, cx| {
            let _ = panel.update(cx, |p, cx| match p.pane_of_conversation(&id) {
                Some(pane) => p.reveal_pane(pane, window, cx),
                None => {
                    p.open_terminal(cwd, Some(format!("{command}\r")), window, cx);
                    if let Some(ws) = p.workspace() {
                        ws.update(cx, |ws, cx| ws.set_open(Slot::Bottom, true, cx));
                    }
                    p.focus_active(window, cx);
                }
            });
        });
    }
}

impl Render for ConversationPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let now = gitviber_ui::widgets::now_unix();
        let open: Vec<bool> = match &self.list {
            Some(Ok(list)) => list
                .iter()
                .map(|c| {
                    self.panel
                        .upgrade()
                        .is_some_and(|p| p.read(cx).pane_of_conversation(&c.id).is_some())
                })
                .collect(),
            _ => vec![],
        };
        let items: Vec<CommandItem> = match &self.list {
            Some(Ok(list)) => list
                .iter()
                .zip(open)
                .map(|(c, open)| {
                    let (title, branch, when) = (
                        SharedString::from(c.title.clone()),
                        c.branch.clone(),
                        gitviber_ui::widgets::relative_time(c.modified as i64, now),
                    );
                    let mono = t.fonts.code.clone();
                    CommandItem::new().label(title.clone()).child(move |_, cx| {
                        let t = cx.app_theme();
                        let muted = t.muted_foreground;
                        h_flex()
                            .gap_2()
                            .w_full()
                            .min_w_0()
                            .child(div().flex_1().min_w_0().truncate().child(title.clone()))
                            .when(open, |d| {
                                d.child(
                                    div()
                                        .flex_none()
                                        .text_size(rems(0.65625))
                                        .text_color(muted)
                                        .child("open in a terminal"),
                                )
                            })
                            .when_some(branch.clone(), |d, b| {
                                d.child(
                                    div()
                                        .flex_none()
                                        .max_w(px(128.))
                                        .truncate()
                                        .font_family(mono.clone())
                                        .text_size(rems(0.65625))
                                        .text_color(muted)
                                        .child(b),
                                )
                            })
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(rems(0.65625))
                                    .text_color(muted)
                                    .child(when.clone()),
                            )
                    })
                })
                .collect(),
            _ => vec![],
        };
        let empty: SharedString = match &self.list {
            None => "Looking for conversations…".into(),
            Some(Err(e)) => format!("Could not read the conversations: {e}").into(),
            Some(Ok(l)) if l.is_empty() => format!(
                "No Claude Code conversations in {} yet",
                folder_name(&self.cwd)
            )
            .into(),
            Some(Ok(_)) => "No matching conversations".into(),
        };
        let this = cx.weak_entity();
        let placeholder = format!("Resume a conversation in {}", folder_name(&self.cwd));
        v_flex()
            .child(
                div()
                    .px_3()
                    .pt_2()
                    .text_size(rems(0.6875))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.muted_foreground)
                    .child("Resume a Conversation"),
            )
            .child(
                Command::new(&self.state)
                    .filterable(true)
                    .bordered(false)
                    .max_h(px(420.))
                    .placeholder(placeholder)
                    .items(items)
                    .empty(move |_, _, cx| {
                        div()
                            .py_3()
                            .text_center()
                            .text_color(cx.app_theme().subtle)
                            .child(empty.clone())
                    })
                    .on_confirm(move |index, window, cx| {
                        let _ = this.update(cx, |p, cx| p.confirm(index.row, window, cx));
                    }),
            )
    }
}
