//! The command palette (⇧⌘P) and quick open (⌘P), one box: a query starting with ">" lists the
//! commands that apply where focus was, anything else the quick open source's rows. Built when
//! it opens and dropped when it closes. What was picked runs once the dialog is gone, from
//! where focus was, so it lands where the user was working.

use gitviber_ui::fuzzy::{Match, fuzzy_match, match_path, prepare_query};
use gitviber_ui::keymap::{self, Category, IS_MAC, bindings_for, format_chord};
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::FileIcon;
use gitviber_ui::workspace::{PanelContext, QuickItem, Registry, RunItem, Slot, Workspace};
use gitviber_ui::{ActiveAppTheme, Settings, UiState};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::command::{Command, CommandItem, CommandState};
use gpui_kit::{
    Action, App, AppContext as _, Context, Entity, FocusHandle, FontWeight, HighlightStyle,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, StyledText, Window, div,
    prelude::*, px,
};

/// Rows shown at most; the rest are a narrower query away.
const LIMIT: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Commands,
    Files,
}

enum Run {
    Command {
        id: &'static str,
        action: Box<dyn Action>,
    },
    Quick(RunItem),
}

struct Row {
    label: SharedString,
    hits: Vec<usize>,
    hint: Option<SharedString>,
    path: bool,
    run: Run,
}

pub struct Palette {
    state: Entity<CommandState>,
    query: SharedString,
    /// The commands that applied where focus was, with their titles as listed.
    commands: Vec<(&'static keymap::Command, SharedString)>,
    quick: Option<Vec<QuickItem>>,
    rows: Vec<Row>,
    focus_return: Option<FocusHandle>,
    _live: Live,
}

fn command_label(c: &keymap::Command, workspace: Option<&Workspace>) -> SharedString {
    let title = match (c.id, workspace) {
        ("terminal.toggle", Some(ws)) if ws.is_open(Slot::Bottom) => "Hide Terminal",
        ("terminal.toggle", Some(_)) => "Show Terminal",
        ("terminal.toggleMaximize", Some(ws)) if ws.terminal_maximized() => {
            "Exit Maximized Terminal"
        }
        _ => c.title,
    };
    if c.category == Category::General {
        title.into()
    } else {
        format!("{}: {title}", c.category.label()).into()
    }
}

/// Best matches first; with an empty query, recent ones first, then the list's own order.
fn rank<T>(
    query: &str,
    list: &[T],
    text: impl Fn(&T) -> &str,
    path: bool,
    recency: impl Fn(&T) -> Option<usize>,
) -> Vec<(usize, Match)> {
    let q = prepare_query(query);
    let mut out: Vec<(usize, Match, usize)> = list
        .iter()
        .enumerate()
        .filter_map(|(at, item)| {
            let m = if path {
                match_path(&q, text(item))
            } else {
                fuzzy_match(&q, text(item))
            }?;
            Some((at, m, recency(item).unwrap_or(usize::MAX)))
        })
        .collect();
    out.sort_by(|a, b| {
        let by_score = if q.is_empty() {
            std::cmp::Ordering::Equal
        } else {
            b.1.score.total_cmp(&a.1.score)
        };
        by_score.then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0))
    });
    out.truncate(LIMIT);
    out.into_iter().map(|(at, m, _)| (at, m)).collect()
}

impl Palette {
    /// Opens the palette over `window`, listing what applies where focus is now.
    pub fn open(mode: Mode, workspace: &Entity<Workspace>, window: &mut Window, cx: &mut App) {
        if window.has_active_dialog(cx) {
            return;
        }
        let ws = workspace.read(cx);
        let available = window.available_actions(cx);
        let mut commands: Vec<(&'static keymap::Command, SharedString)> = keymap::all_commands()
            .filter(|c| c.id != "workbench.showCommands")
            .filter(|c| {
                let name = c.action().name();
                available.iter().any(|a| a.name() == name)
            })
            .map(|c| (c, command_label(c, Some(ws))))
            .collect();
        commands.sort_by_key(|(c, _)| keymap::all_commands().position(|o| o.id == c.id));
        let ctx = PanelContext {
            workspace: workspace.downgrade(),
            project: ws.project().cloned(),
        };
        let quick = Registry::get(cx)
            .quick_open()
            .map(|source| source(&ctx, cx));
        let focus_return = window.focused(cx);
        let initial: SharedString = if mode == Mode::Commands {
            ">".into()
        } else {
            "".into()
        };
        let palette = cx.new(|cx| {
            let state = cx.new(|cx| CommandState::new(window, cx));
            state.update(cx, |s, cx| s.set_query(initial.clone(), window, cx));
            let mut palette = Palette {
                state,
                query: initial,
                commands,
                quick,
                rows: Vec::new(),
                focus_return,
                _live: Live::new("Palette"),
            };
            palette.rank(cx);
            palette
        });
        let first = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                let state = palette.read(cx).state.clone();
                state.update(cx, |s, cx| s.focus(window, cx));
            }
            dialog
                .w(px(576.))
                .margin_top(px(90.))
                .close_button(false)
                .p_0()
                .child(palette.clone())
        });
    }

    fn rank(&mut self, cx: &App) {
        let query = self.query.to_string();
        if let Some(q) = query.strip_prefix('>') {
            let recent = &UiState::get(cx).recent_commands;
            let overrides = &Settings::get(cx).keybindings;
            self.rows = rank(
                q,
                &self.commands,
                |(_, label)| label.as_ref(),
                false,
                |(c, _)| recent.iter().position(|r| r == c.id),
            )
            .into_iter()
            .map(|(at, m)| {
                let (c, label) = &self.commands[at];
                let chord = bindings_for(c, overrides, IS_MAC).into_iter().next();
                let recent_tag = q.trim().is_empty() && recent.iter().any(|r| r == c.id);
                Row {
                    label: label.clone(),
                    hits: m.hits,
                    hint: chord
                        .map(|k| format_chord(&k, IS_MAC).into())
                        .or(recent_tag.then(|| "recently used".into())),
                    path: false,
                    run: Run::Command {
                        id: c.id,
                        action: c.action(),
                    },
                }
            })
            .collect();
            return;
        }
        let Some(items) = &self.quick else {
            self.rows.clear();
            return;
        };
        self.rows = rank(&query, items, |i| i.label.as_ref(), true, |_| None)
            .into_iter()
            .map(|(at, m)| {
                let item = &items[at];
                Row {
                    label: item.label.clone(),
                    hits: m.hits,
                    hint: item.detail.clone(),
                    path: item.is_path,
                    run: Run::Quick(item.run.clone()),
                }
            })
            .collect();
    }

    fn confirm(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(row) else {
            return;
        };
        let focus = self.focus_return.clone();
        window.close_dialog(cx);
        match &row.run {
            Run::Command { id, action } => {
                let id = *id;
                UiState::update(cx, |s| s.add_recent_command(id));
                let action = action.boxed_clone();
                window.defer(cx, move |window, cx| match focus {
                    Some(focus) => {
                        focus.focus(window, cx);
                        focus.dispatch_action(action.as_ref(), window, cx);
                    }
                    None => window.dispatch_action(action, cx),
                });
            }
            Run::Quick(run) => {
                let run = run.clone();
                window.defer(cx, move |window, cx| run(window, cx));
            }
        }
    }

    fn empty_text(&self) -> &'static str {
        if self.query.starts_with('>') {
            "No commands match."
        } else if self.quick.is_none() {
            "Quick open lists files once gitviber-files is in. Type > for commands."
        } else {
            "No files match."
        }
    }
}

/// `text` with the matched characters (char indices) in semibold.
fn highlighted(text: &SharedString, hits: &[usize]) -> StyledText {
    let bold = HighlightStyle {
        font_weight: Some(FontWeight::SEMIBOLD),
        ..Default::default()
    };
    let ranges: Vec<_> = text
        .char_indices()
        .enumerate()
        .filter(|(i, _)| hits.contains(i))
        .map(|(_, (b, c))| (b..b + c.len_utf8(), bold))
        .collect();
    StyledText::new(text.clone()).with_highlights(ranges)
}

impl Render for Palette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let items: Vec<CommandItem> = self
            .rows
            .iter()
            .map(|row| {
                let (label, hits, hint, path) = (
                    row.label.clone(),
                    row.hits.clone(),
                    row.hint.clone(),
                    row.path,
                );
                let mono = t.fonts.code.clone();
                CommandItem::new().label(label.clone()).child(move |_, cx| {
                    let t = cx.app_theme();
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .min_w_0()
                        .when(path, |d| d.child(FileIcon::file(&label)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(highlighted(&label, &hits)),
                        )
                        .when_some(hint.clone(), |d, hint| {
                            d.child(
                                div()
                                    .flex_none()
                                    .font_family(mono.clone())
                                    .text_size(gpui_kit::rems(0.6875))
                                    .text_color(t.muted_foreground)
                                    .child(hint),
                            )
                        })
                })
            })
            .collect();
        let this = cx.entity().downgrade();
        let this_confirm = this.clone();
        let empty = self.empty_text();
        Command::new(&self.state)
            .filterable(false)
            .bordered(false)
            .max_h(px(420.))
            .placeholder("Type a command, or a file name")
            .items(items)
            .empty(move |_, _, cx| {
                div()
                    .py_3()
                    .text_center()
                    .text_color(cx.app_theme().subtle)
                    .child(empty)
            })
            .on_query(move |query, _, cx| {
                let query = SharedString::from(query.to_string());
                let _ = this.update(cx, |p, cx| {
                    p.query = query;
                    p.rank(cx);
                    cx.notify();
                });
            })
            .on_confirm(move |index, window, cx| {
                let _ = this_confirm.update(cx, |p, cx| p.confirm(index.row, window, cx));
            })
    }
}
