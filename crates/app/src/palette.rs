//! The command palette (⇧⌘P) and quick open (⌘P), one box: a query starting with ">" lists the
//! commands that apply where focus was, anything else the mode's rows: files (recently opened
//! first), the changed files (Open Changed File), the other projects to start a terminal in (New
//! Terminal in Project…), or an agent's past conversations in the worktree (Resume a
//! Conversation…). Built when it opens and dropped when it closes. What was picked runs once the
//! dialog is gone, from where focus was, so it lands where the user was working.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use gitviber_core::conversations::Conversation;
use gitviber_core::git::FileChange;
use gitviber_core::{Core, api};
use gitviber_ui::fuzzy::{Match, fuzzy_match, match_path, prepare_query};
use gitviber_ui::keymap::{self, Category, IS_MAC, ROOT_CONTEXT, bindings_for, format_chord};
use gitviber_ui::memory::Live;
use gitviber_ui::state::bump;
use gitviber_ui::widgets::{ChangeKind, FileIcon, StatusLetter, relative_time};
use gitviber_ui::workspace::{
    PanelContext, QuickItem, Registry, RunItem, Selection, Slot, Workspace,
};
use gitviber_ui::{ActiveAppTheme, Settings, UiState, actions::workbench, store};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::command::{Command, CommandItem, CommandState};
use gpui_kit::{
    Action, App, AppContext as _, Context, Entity, FocusHandle, FontWeight, Global, HighlightStyle,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    StyledText, Task, WeakEntity, Window, div, prelude::*, px,
};
use serde_json::Value;

/// Rows shown at most; the best matches are at the top, and a repo can have 100k files.
const LIMIT: usize = 200;
/// Files remembered per repository.
const RECENT_FILES: usize = 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Commands,
    Files,
    Changes,
    Projects,
    Conversations,
}

enum Run {
    Command {
        id: &'static str,
        action: Box<dyn Action>,
    },
    Quick(RunItem),
    /// A file of quick open's: remembered as recently opened.
    File {
        path: SharedString,
        run: RunItem,
    },
    Change(Selection),
    Terminal {
        cwd: PathBuf,
        command: Option<String>,
    },
}

struct Row {
    label: SharedString,
    hits: Vec<usize>,
    hint: Option<SharedString>,
    path: bool,
    /// A change's status letter.
    status: Option<SharedString>,
    run: Run,
}

/// The palette's own file: files recently opened from it, per worktree, as the old palette kept
/// them (gitviber.palette.files).
#[derive(Default)]
struct RecentFiles(BTreeMap<String, Vec<String>>);
impl Global for RecentFiles {}

fn recent_path() -> PathBuf {
    store::config_dir().join("palette.json")
}

fn recent_files(cx: &mut App) -> &mut RecentFiles {
    if !cx.has_global::<RecentFiles>() {
        let read = store::read_object(&recent_path())
            .and_then(|m| m.get("files").cloned())
            .and_then(|v| serde_json::from_value::<BTreeMap<String, Vec<String>>>(v).ok())
            .unwrap_or_default();
        cx.set_global(RecentFiles(read));
    }
    cx.global_mut::<RecentFiles>()
}

fn remember_file(root: &str, path: &str, cx: &mut App) {
    let files = recent_files(cx);
    bump(
        files.0.entry(root.to_string()).or_default(),
        path,
        RECENT_FILES,
    );
    let mut map = serde_json::Map::new();
    map.insert(
        "files".into(),
        serde_json::to_value(&files.0).unwrap_or(Value::Null),
    );
    store::save_object(cx, recent_path(), map);
}

pub struct Palette {
    state: Entity<CommandState>,
    mode: Mode,
    query: SharedString,
    workspace: WeakEntity<Workspace>,
    /// The worktree it opened on, for recent files and conversations.
    root: Option<PathBuf>,
    /// The commands that applied where focus was, with their titles as listed.
    commands: Vec<(&'static keymap::Command, SharedString)>,
    quick: Option<Vec<QuickItem>>,
    recent: Vec<String>,
    changes: Option<Vec<(Selection, Arc<FileChange>, &'static str)>>,
    projects: Vec<String>,
    conversations: Option<Vec<Conversation>>,
    error: Option<SharedString>,
    rows: Vec<Row>,
    focus_return: Option<FocusHandle>,
    _load: Option<Task<()>>,
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

/// A worktree's changed files as the changes panel opens them, with their kind's label.
fn changes_of(
    status: gitviber_core::git::RepoStatus,
) -> Vec<(Selection, Arc<FileChange>, &'static str)> {
    let conflicts = status.conflicted.into_iter().map(|f| {
        let f = Arc::new(f);
        (Selection::Conflict { file: f.clone() }, f, "Conflict")
    });
    let staged = status.staged.into_iter().map(|f| {
        let f = Arc::new(f);
        (Selection::Staged { file: f.clone() }, f, "Staged")
    });
    let unstaged = status.unstaged.into_iter().map(|f| {
        let f = Arc::new(f);
        (Selection::Unstaged { file: f.clone() }, f, "")
    });
    conflicts.chain(unstaged).chain(staged).collect()
}

impl Palette {
    /// Opens the palette over `window` in `mode`, listing what applies where focus is now.
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
        let project = ws.project().cloned();
        let root = project.as_ref().map(|p| p.read(cx).root().to_path_buf());
        let ctx = PanelContext {
            workspace: workspace.downgrade(),
            project: project.clone(),
        };
        let quick = (mode == Mode::Files)
            .then(|| {
                Registry::get(cx)
                    .quick_open()
                    .map(|source| source(&ctx, cx))
            })
            .flatten();
        let main = project.as_ref().and_then(|p| {
            p.read(cx)
                .main_worktree()
                .map(|m| m.to_string_lossy().into_owned())
        });
        let projects: Vec<String> = UiState::get(cx)
            .recent_projects
            .iter()
            .filter(|p| main.as_deref() != Some(p.as_str()))
            .cloned()
            .collect();
        let recent = root
            .as_ref()
            .and_then(|r| recent_files(cx).0.get(&*r.to_string_lossy()).cloned())
            .unwrap_or_default();
        let focus_return = window.focused(cx);
        let initial: SharedString = if mode == Mode::Commands {
            ">".into()
        } else {
            "".into()
        };
        let weak_ws = workspace.downgrade();
        let core = gitviber_ui::has_core(cx).then(|| gitviber_ui::core(cx));
        let palette = cx.new(|cx| {
            let state = cx.new(|cx| CommandState::new(window, cx));
            state.update(cx, |s, cx| s.set_query(initial.clone(), window, cx));
            let mut palette = Palette {
                state,
                mode,
                query: initial,
                workspace: weak_ws,
                root,
                commands,
                quick,
                recent,
                changes: None,
                projects,
                conversations: None,
                error: None,
                rows: Vec::new(),
                focus_return,
                _load: None,
                _live: Live::new("Palette"),
            };
            if let Some(core) = core {
                palette.load(core, cx);
            }
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

    /// What a mode reads off the UI thread: the status, the conversations, read each time it's
    /// asked for (a conversation started a minute ago is the likely one).
    fn load(&mut self, core: Arc<Core>, cx: &mut Context<Self>) {
        match self.mode {
            Mode::Changes => {
                let read = cx.background_spawn(async move { api::changes::status(&core) });
                self._load = Some(cx.spawn(async move |this, cx| {
                    let status = read.await;
                    let _ = this.update(cx, |p, cx| {
                        match status {
                            Ok(s) => p.changes = Some(changes_of(s)),
                            Err(e) => p.error = Some(e.into()),
                        }
                        p.rank(cx);
                        cx.notify();
                    });
                }));
            }
            Mode::Conversations => {
                let Some(cwd) = self.root.clone() else { return };
                let cwd = cwd.to_string_lossy().into_owned();
                let read =
                    cx.background_spawn(async move { api::app::agent_conversations(&core, cwd) });
                self._load = Some(cx.spawn(async move |this, cx| {
                    let list = read.await;
                    let _ = this.update(cx, |p, cx| {
                        match list {
                            Ok(l) => p.conversations = Some(l),
                            Err(e) => p.error = Some(e.into()),
                        }
                        p.rank(cx);
                        cx.notify();
                    });
                }));
            }
            _ => {}
        }
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
                    status: None,
                    run: Run::Command {
                        id: c.id,
                        action: c.action(),
                    },
                }
            })
            .collect();
            return;
        }
        self.rows = match self.mode {
            Mode::Commands | Mode::Files => self.file_rows(&query),
            Mode::Changes => {
                let list = self.changes.as_deref().unwrap_or_default();
                rank(&query, list, |(_, f, _)| f.path.as_str(), true, |_| None)
                    .into_iter()
                    .map(|(at, m)| {
                        let (sel, f, kind) = &list[at];
                        Row {
                            label: f.path.clone().into(),
                            hits: m.hits,
                            hint: (!kind.is_empty()).then(|| SharedString::from(*kind)),
                            path: true,
                            status: Some(f.status.clone().into()),
                            run: Run::Change(sel.clone()),
                        }
                    })
                    .collect()
            }
            Mode::Projects => rank(&query, &self.projects, |p| p.as_str(), true, |_| None)
                .into_iter()
                .map(|(at, m)| {
                    let path = &self.projects[at];
                    Row {
                        label: path.clone().into(),
                        hits: m.hits,
                        hint: None,
                        path: false,
                        status: None,
                        run: Run::Terminal {
                            cwd: path.into(),
                            command: None,
                        },
                    }
                })
                .collect(),
            Mode::Conversations => {
                let list = self.conversations.as_deref().unwrap_or_default();
                let now = gitviber_ui::widgets::now_unix();
                rank(&query, list, |c| c.title.as_str(), false, |_| None)
                    .into_iter()
                    .map(|(at, m)| {
                        let c = &list[at];
                        let when = relative_time(c.modified as i64, now);
                        Row {
                            label: c.title.clone().into(),
                            hits: m.hits,
                            hint: Some(match &c.branch {
                                Some(b) => format!("{b} · {when}").into(),
                                None => when.into(),
                            }),
                            path: false,
                            status: None,
                            run: Run::Terminal {
                                cwd: self.root.clone().unwrap_or_default(),
                                command: Some(c.command.clone()),
                            },
                        }
                    })
                    .collect()
            }
        };
    }

    fn file_rows(&self, query: &str) -> Vec<Row> {
        let Some(items) = &self.quick else {
            return Vec::new();
        };
        let recent = &self.recent;
        rank(
            query,
            items,
            |i| i.label.as_ref(),
            true,
            |i| recent.iter().position(|r| r.as_str() == i.label.as_ref()),
        )
        .into_iter()
        .map(|(at, m)| {
            let item = &items[at];
            let recently =
                query.trim().is_empty() && recent.iter().any(|r| r.as_str() == item.label.as_ref());
            Row {
                label: item.label.clone(),
                hits: m.hits,
                hint: item
                    .detail
                    .clone()
                    .or(recently.then(|| "recently opened".into())),
                path: item.is_path,
                status: None,
                run: if item.is_path {
                    Run::File {
                        path: item.label.clone(),
                        run: item.run.clone(),
                    }
                } else {
                    Run::Quick(item.run.clone())
                },
            }
        })
        .collect()
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
            Run::File { path, run } => {
                if let Some(root) = &self.root {
                    remember_file(&root.to_string_lossy(), path, cx);
                }
                let run = run.clone();
                window.defer(cx, move |window, cx| run(window, cx));
            }
            Run::Change(selection) => {
                let (selection, ws) = (selection.clone(), self.workspace.clone());
                window.defer(cx, move |window, cx| {
                    if let Some(ws) = ws.upgrade() {
                        ws.update(cx, |ws, cx| ws.open(selection, false, window, cx));
                    }
                });
            }
            Run::Terminal { cwd, command } => {
                let (cwd, command, ws) = (cwd.clone(), command.clone(), self.workspace.clone());
                window.defer(cx, move |window, cx| {
                    if let Some(ws) = ws.upgrade() {
                        open_terminal(&ws, cwd, command, window, cx);
                    }
                });
            }
        }
    }

    fn switch(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        let query: SharedString = if mode == Mode::Commands {
            ">".into()
        } else {
            "".into()
        };
        if mode != Mode::Commands && self.quick.is_none() && mode == Mode::Files {
            let ctx = PanelContext {
                workspace: self.workspace.clone(),
                project: self
                    .workspace
                    .upgrade()
                    .and_then(|w| w.read(cx).project().cloned()),
            };
            self.quick = Registry::get(cx)
                .quick_open()
                .map(|source| source(&ctx, cx));
        }
        if mode != Mode::Commands {
            self.mode = mode;
        }
        self.query = query.clone();
        self.state
            .update(cx, |s, cx| s.set_query(query, window, cx));
        self.rank(cx);
        cx.notify();
    }

    fn empty_text(&self) -> SharedString {
        if self.query.starts_with('>') {
            return "No matching commands".into();
        }
        if let Some(e) = &self.error {
            return e.clone();
        }
        match self.mode {
            Mode::Conversations => match &self.conversations {
                None => "Looking for conversations…".into(),
                Some(l) if l.is_empty() => {
                    let name = self
                        .root
                        .as_ref()
                        .and_then(|r| r.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    format!("No Claude Code conversations in {name} yet").into()
                }
                Some(_) => "No matching conversations".into(),
            },
            Mode::Changes => match &self.changes {
                None => "Reading the changes…".into(),
                Some(l) if l.is_empty() => "No changes".into(),
                Some(_) => "No matching changes".into(),
            },
            Mode::Projects => "No matching projects".into(),
            _ if self.root.is_none() => {
                "Open a repository to find its files · type > for commands".into()
            }
            _ if self.quick.is_none() => "Listing files…".into(),
            _ => "No matching files".into(),
        }
    }

    fn placeholder(&self) -> &'static str {
        if self.query.starts_with('>') {
            return "Type a command";
        }
        match self.mode {
            Mode::Changes => "Open a changed file's diff · type > for commands",
            Mode::Projects => "Open a terminal in a project · type > for commands",
            Mode::Conversations => "Resume a conversation · type > for commands",
            _ => "Search files by name · type > for commands",
        }
    }
}

/// A new terminal tab in `cwd`, typing `command` at its prompt, the window staying where it is.
pub fn open_terminal(
    ws: &Entity<Workspace>,
    cwd: PathBuf,
    command: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(panel) = Workspace::ensure_panel(ws, "terminal", window, cx)
        .and_then(|p| p.view().downcast::<gitviber_terminal::TerminalPanel>().ok())
    else {
        return;
    };
    ws.update(cx, |ws, cx| ws.set_open(Slot::Bottom, true, cx));
    panel.update(cx, |p, cx| p.open_terminal(cwd, command, window, cx));
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
                let (label, hits, hint, path, status) = (
                    row.label.clone(),
                    row.hits.clone(),
                    row.hint.clone(),
                    row.path,
                    row.status.clone(),
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
                        .when_some(status.clone(), |d, s| {
                            let letter = s.chars().next().unwrap_or('M');
                            d.child(StatusLetter(ChangeKind::from_letter(letter)))
                        })
                })
            })
            .collect();
        let this = cx.entity().downgrade();
        let this_confirm = this.clone();
        let empty = self.empty_text();
        div()
            // The palette's own keys switch it over, and the app's other keys stay out.
            .key_context(ROOT_CONTEXT)
            .on_action(
                cx.listener(|this, _: &workbench::ShowCommands, window, cx| {
                    this.switch(Mode::Commands, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &workbench::QuickOpen, window, cx| {
                if this.root.is_some() {
                    this.switch(Mode::Files, window, cx)
                }
            }))
            .child(
                Command::new(&self.state)
                    .filterable(false)
                    .bordered(false)
                    .max_h(px(420.))
                    .placeholder(self.placeholder())
                    .items(items)
                    .empty(move |_, _, cx| {
                        div()
                            .py_3()
                            .text_center()
                            .text_color(cx.app_theme().subtle)
                            .child(empty.clone())
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
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_ones_come_first_with_an_empty_query() {
        let list = ["a.rs", "b.rs", "c.rs"];
        let recency = |s: &&str| match *s {
            "c.rs" => Some(0),
            "b.rs" => Some(1),
            _ => None,
        };
        let order: Vec<usize> = rank("", &list, |s| s, true, recency)
            .into_iter()
            .map(|(at, _)| at)
            .collect();
        assert_eq!(order, [2, 1, 0]);
        // A query ranks by score first.
        let order: Vec<usize> = rank("a", &list, |s| s, true, recency)
            .into_iter()
            .map(|(at, _)| at)
            .collect();
        assert_eq!(order[0], 0);
    }
}
