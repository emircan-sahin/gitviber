//! The command palette (⇧⌘P) and quick open (⌘P), one box: a query starting with ">" lists the
//! commands that apply where focus was, anything else the mode's rows: files (recently opened
//! first), the changed files (Open Changed File) or the other projects to start a terminal in (New
//! Terminal in Project…). Built when it opens and dropped when it closes. What was picked runs once
//! the dialog is gone, from where focus was, so it lands where the user was working.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use gitviber_core::git::{FileChange, GitInfo};
use gitviber_core::{Core, api};
use gitviber_files::quick_open::{self, QuickOpenFiles, QuickOpenStatus};
use gitviber_ui::fuzzy::{Match, fuzzy_match, match_path, prepare_query};
use gitviber_ui::keymap::{self, Category, IS_MAC, ROOT_CONTEXT, bindings_for, format_chord};
use gitviber_ui::memory::Live;
use gitviber_ui::state::bump;
use gitviber_ui::widgets::{ChangeKind, FileIcon, StatusLetter};
use gitviber_ui::workspace::{
    PanelContext, QuickItem, Registry, RunItem, Selection, Slot, Workspace,
};
use gitviber_ui::{ActiveAppTheme, Settings, UiState, actions::workbench, store};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::command::{Command, CommandItem, CommandState};
use gpui_kit::component::dialog::DialogEntrance;
use gpui_kit::{
    Action, App, AppContext as _, Context, Entity, FocusHandle, FontWeight, Global, HighlightStyle,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString,
    Styled as _, StyledText, Subscription, Task, WeakEntity, Window, div, prelude::*, px,
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

/// The old app's recent files (gitviber.palette.files: root → paths, the latest first), into
/// palette.json on the first launch, while the file isn't there yet.
pub fn import_legacy() {
    let path = recent_path();
    if path.exists() {
        return;
    }
    if let Some(files) = legacy_files(gitviber_ui::legacy::value("gitviber.palette.files")) {
        let text = serde_json::json!({ "files": files }).to_string();
        if let Err(err) = store::write_atomic(&path, &text) {
            eprintln!("gitviber: couldn't write {}: {err}", path.display());
        }
    }
}

/// Roots with their paths kept, other values dropped, each list capped as the palette keeps it.
fn legacy_files(value: Option<Value>) -> Option<BTreeMap<String, Vec<String>>> {
    let map = value?.as_object()?.clone();
    let files: BTreeMap<String, Vec<String>> = map
        .into_iter()
        .filter_map(|(root, list)| {
            let paths: Vec<String> = list
                .as_array()?
                .iter()
                .filter_map(|p| p.as_str().map(str::to_string))
                .take(RECENT_FILES)
                .collect();
            (!paths.is_empty()).then_some((root, paths))
        })
        .collect();
    (!files.is_empty()).then_some(files)
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
    /// The worktree it opened on, for recent files.
    root: Option<PathBuf>,
    /// The commands that applied where focus was.
    commands: Vec<Listed>,
    quick: Option<Vec<QuickItem>>,
    /// Where quick open's list stood when `quick` was asked for.
    listing: Option<QuickOpenStatus>,
    recent: Vec<String>,
    changes: Option<Vec<(Selection, Arc<FileChange>, &'static str)>>,
    projects: Vec<String>,
    error: Option<SharedString>,
    rows: Vec<Row>,
    focus_return: Option<FocusHandle>,
    _load: Option<Task<()>>,
    /// Quick open's lists, observed once files were asked for: a listing landing shows up.
    _listed: Option<Subscription>,
    _live: Live,
}

/// A command as the palette lists it: its title alone, as the menus and shortcut lists name it
/// (the titles are unique and say their area: "Toggle Terminal", "Reload Page"). `search` puts
/// the category first ("Git Push"), so typing an area still finds its commands.
struct Listed {
    command: &'static keymap::Command,
    label: SharedString,
    search: String,
}

impl Listed {
    fn new(command: &'static keymap::Command, workspace: Option<&Workspace>) -> Self {
        let label = command_label(command, workspace);
        let search = if command.category == Category::General {
            label.to_string()
        } else {
            format!("{} {label}", command.category.label())
        };
        Self {
            command,
            label,
            search,
        }
    }

    /// `search`'s matched chars as the label's: those in the category aren't drawn.
    fn label_hits(&self, hits: Vec<usize>) -> Vec<usize> {
        let skip = self.search.chars().count() - self.label.chars().count();
        hits.into_iter()
            .filter_map(|h| h.checked_sub(skip))
            .collect()
    }
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
    title.into()
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
        Self::show(mode, workspace, window, cx);
    }

    /// `open`, giving the palette back; None while another dialog is up.
    fn show(
        mode: Mode,
        workspace: &Entity<Workspace>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Entity<Self>> {
        if window.has_active_dialog(cx) {
            return None;
        }
        let ws = workspace.read(cx);
        let available = window.available_actions(cx);
        let commands: Vec<Listed> = keymap::all_commands()
            .filter(|c| c.id != "workbench.showCommands")
            .filter(|c| {
                let name = c.action().name();
                available.iter().any(|a| a.name() == name)
            })
            .map(|c| Listed::new(c, Some(ws)))
            .collect();
        let project = ws.project().cloned();
        let root = project.as_ref().map(|p| p.read(cx).root().to_path_buf());
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
                quick: None,
                listing: None,
                recent,
                changes: None,
                projects,
                error: None,
                rows: Vec::new(),
                focus_return,
                _load: None,
                _listed: None,
                _live: Live::new("Palette"),
            };
            if mode == Mode::Files {
                palette.list_files(cx);
            }
            if let Some(core) = core {
                palette.load(core, cx);
            }
            palette.rank(cx);
            palette
        });
        let shown = palette.clone();
        // No entrance animation: the palette is there, whole, on the frame after the key.
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .w(px(576.))
                .margin_top(px(90.))
                .close_button(false)
                .entrance(DialogEntrance::None)
                .p_0()
                .child(shown.clone())
        });
        // Focused here: focusing while the dialog draws asks for one more frame.
        let state = palette.read(cx).state.clone();
        state.update(cx, |s, cx| s.focus(window, cx));
        Some(palette)
    }

    fn panel_context(&self, cx: &App) -> PanelContext {
        PanelContext {
            workspace: self.workspace.clone(),
            project: self
                .workspace
                .upgrade()
                .and_then(|w| w.read(cx).project().cloned()),
        }
    }

    /// Quick open's files as it has them now, never waiting on git; a listing that lands while
    /// the palette is open comes in through `listed`.
    fn list_files(&mut self, cx: &mut Context<Self>) {
        let ctx = self.panel_context(cx);
        self.quick = Registry::get(cx)
            .quick_open()
            .map(|source| source(&ctx, cx));
        self.listing = Some(quick_open::status(&ctx, cx));
        if self._listed.is_none() {
            self._listed = Some(cx.observe_global::<QuickOpenFiles>(|p, cx| p.listed(cx)));
        }
    }

    /// Quick open's lists changed. Asking its source writes that global again, so it's asked
    /// only when this project's status moved (a listing landed or failed), or this would loop.
    fn listed(&mut self, cx: &mut Context<Self>) {
        let ctx = self.panel_context(cx);
        if self.listing.as_ref() == Some(&quick_open::status(&ctx, cx)) {
            return;
        }
        self.list_files(cx);
        self.rank(cx);
        cx.notify();
    }

    /// Open Changed File's list, read off the UI thread each time it's asked for.
    fn load(&mut self, core: Arc<Core>, cx: &mut Context<Self>) {
        if self.mode != Mode::Changes {
            return;
        }
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

    fn rank(&mut self, cx: &App) {
        let query = self.query.to_string();
        if let Some(q) = query.strip_prefix('>') {
            let recent = &UiState::get(cx).recent_commands;
            let overrides = &Settings::get(cx).keybindings;
            self.rows = rank(
                q,
                &self.commands,
                |l| l.search.as_str(),
                false,
                |l| recent.iter().position(|r| r == l.command.id),
            )
            .into_iter()
            .map(|(at, m)| {
                let listed = &self.commands[at];
                let (c, label) = (listed.command, &listed.label);
                let chord = bindings_for(c, overrides, IS_MAC).into_iter().next();
                let recent_tag = q.trim().is_empty() && recent.iter().any(|r| r == c.id);
                Row {
                    label: label.clone(),
                    hits: listed.label_hits(m.hits),
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
                        run: Run::Terminal { cwd: path.into() },
                    }
                })
                .collect(),
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
                let (run, ws) = (run.clone(), self.workspace.clone());
                window.defer(cx, move |window, cx| {
                    run(window, cx);
                    if let Some(ws) = ws.upgrade() {
                        ws.update(cx, |ws, cx| ws.focus_center(window, cx));
                    }
                });
            }
            Run::Change(selection) => {
                let (selection, ws) = (selection.clone(), self.workspace.clone());
                window.defer(cx, move |window, cx| {
                    if let Some(ws) = ws.upgrade() {
                        ws.update(cx, |ws, cx| {
                            if ws.open(selection, false, window, cx) {
                                ws.focus_center(window, cx);
                            }
                        });
                    }
                });
            }
            Run::Terminal { cwd } => {
                let (cwd, ws) = (cwd.clone(), self.workspace.clone());
                window.defer(cx, move |window, cx| {
                    if let Some(ws) = ws.upgrade() {
                        open_terminal(&ws, cwd, window, cx);
                    }
                });
            }
        }
    }

    /// PgUp / PgDn: ten rows at a time, stopping at the ends.
    fn page(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let dir: isize = match k.key.as_str() {
            "pageup" => -10,
            "pagedown" => 10,
            _ => return,
        };
        if k.modifiers.modified() {
            return;
        }
        cx.stop_propagation();
        let state = self.state.read(cx);
        let Some(last) = state.matched_count().checked_sub(1) else {
            return;
        };
        let at = state.selected_index().map_or(0, |i| i.row);
        let to = (at as isize + dir).clamp(0, last as isize) as usize;
        self.state.update(cx, |s, cx| {
            s.set_selected_index(Some(gpui_kit::component::IndexPath::new(to)), window, cx)
        });
    }

    fn switch(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        let query: SharedString = if mode == Mode::Commands {
            ">".into()
        } else {
            "".into()
        };
        if mode == Mode::Files && self.quick.is_none() {
            self.list_files(cx);
        }
        if mode != Mode::Commands && mode != self.mode {
            self.mode = mode;
            self.error = None;
            if gitviber_ui::has_core(cx) {
                self.load(gitviber_ui::core(cx), cx);
            }
        }
        self.query = query.clone();
        self.state
            .update(cx, |s, cx| s.set_query(query, window, cx));
        self.rank(cx);
        cx.notify();
    }

    fn empty_text(&self, cx: &App) -> SharedString {
        if self.query.starts_with('>') {
            return "No matching commands".into();
        }
        if let Some(e) = &self.error {
            return e.clone();
        }
        match self.mode {
            Mode::Changes => match &self.changes {
                None => "Reading the changes…".into(),
                Some(l) if l.is_empty() => "No changes".into(),
                Some(_) => "No matching changes".into(),
            },
            Mode::Projects => "No matching projects".into(),
            _ if self.root.is_none() => {
                "Open a repository to find its files · type > for commands".into()
            }
            _ => match &self.listing {
                None | Some(QuickOpenStatus::Listing) => "Listing files…".into(),
                Some(QuickOpenStatus::Failed(err)) => {
                    unlisted(err, crate::dialogs::needs_git::old(cx)).into()
                }
                Some(QuickOpenStatus::Listed) => "No matching files".into(),
            },
        }
    }

    fn placeholder(&self) -> &'static str {
        if self.query.starts_with('>') {
            return "Type a command";
        }
        match self.mode {
            Mode::Changes => "Open a changed file's diff · type > for commands",
            Mode::Projects => "Open a terminal in a project · type > for commands",
            _ => "Search files by name · type > for commands",
        }
    }
}

/// Why quick open has no files: the old git the launch check found, else git's first line.
fn unlisted(err: &str, old: Option<&GitInfo>) -> String {
    let why = match old {
        Some(git) => format!(
            "git {} is older than GitViber needs ({})",
            git.version.as_deref().unwrap_or_default(),
            git.minimum
        ),
        None => err.lines().next().unwrap_or(err).trim().to_string(),
    };
    format!("Could not list files: {why}")
}

/// A new terminal tab in `cwd`, the window staying where it is.
pub fn open_terminal(ws: &Entity<Workspace>, cwd: PathBuf, window: &mut Window, cx: &mut App) {
    let Some(panel) = Workspace::ensure_panel(ws, "terminal", window, cx)
        .and_then(|p| p.view().downcast::<gitviber_terminal::TerminalPanel>().ok())
    else {
        return;
    };
    ws.update(cx, |ws, cx| ws.set_open(Slot::Bottom, true, cx));
    panel.update(cx, |p, cx| p.open_terminal(cwd, None, window, cx));
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
        let empty = self.empty_text(cx);
        div()
            // The palette's own keys switch it over, and the app's other keys stay out.
            .key_context(ROOT_CONTEXT)
            .on_key_down(cx.listener(Self::page))
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

    use std::rc::Rc;

    use gitviber_ui::workspace::{TabHandle, TabItem, register_opener, register_quick_open};
    use gpui_kit::{TestAppContext, VisualTestContext};

    #[test]
    fn the_old_apps_recent_files_keep_their_roots_and_paths() {
        let old = serde_json::json!({
            "/r/a": ["src/x.rs", 3, "README.md"],
            "/r/b": [],
            "/r/c": "nope"
        });
        let files = legacy_files(Some(old)).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files["/r/a"], ["src/x.rs", "README.md"]);
        assert!(legacy_files(Some(serde_json::json!([1]))).is_none());
        assert!(legacy_files(None).is_none());
    }

    #[test]
    fn commands_read_without_a_prefix_and_are_found_by_their_area() {
        let by_id = |id| keymap::all_commands().find(|c| c.id == id).unwrap();
        let push = Listed::new(by_id("git.push"), None);
        assert_eq!(push.label.as_ref(), "Push");
        assert_eq!(push.search, "Git Push");
        let m = fuzzy_match(&prepare_query("git push"), &push.search).unwrap();
        assert_eq!(push.label_hits(m.hits), [0, 1, 2, 3]);
        let open = Listed::new(by_id("workbench.quickOpen"), None);
        assert_eq!(open.label.as_ref(), open.search);
        // Unique titles are what lets the palette drop the prefix.
        let mut titles: Vec<_> = keymap::all_commands().map(|c| c.title).collect();
        titles.sort_unstable();
        let n = titles.len();
        titles.dedup();
        assert_eq!(titles.len(), n);
    }

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

    #[test]
    fn files_that_cant_be_listed_say_why() {
        let err = "error: unknown option `deduplicate'\nusage: git ls-files [<options>]";
        assert_eq!(
            unlisted(err, None),
            "Could not list files: error: unknown option `deduplicate'"
        );
        let old = GitInfo {
            state: "old",
            version: Some("2.15.0".into()),
            detail: None,
            minimum: "2.36".into(),
        };
        assert_eq!(
            unlisted(err, Some(&old)),
            "Could not list files: git 2.15.0 is older than GitViber needs (2.36)"
        );
    }

    struct Tab {
        focus: FocusHandle,
    }
    impl Render for Tab {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().track_focus(&self.focus)
        }
    }
    impl TabItem for Tab {
        fn tab_title(&self, _: &App) -> SharedString {
            "tab".into()
        }
        fn tab_focus(&self, _: &App) -> Option<FocusHandle> {
            Some(self.focus.clone())
        }
    }

    /// The main window as the palette sees it: the active tab under a root that holds focus.
    struct Host {
        ws: Entity<Workspace>,
        focus: FocusHandle,
    }
    impl Render for Host {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let view = self.ws.update(cx, |ws, cx| ws.active_view(window, cx));
            div().size_full().track_focus(&self.focus).children(view)
        }
    }

    fn window<'a>(
        name: &str,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Host>, &'a mut VisualTestContext) {
        let dir =
            std::env::temp_dir().join(format!("gitviber-palette-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(gitviber_ui::init);
        let mut host = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let ws = cx.new(Workspace::new);
            Workspace::register_window(&ws, window, cx);
            let h = cx.new(|cx| Host {
                ws,
                focus: cx.focus_handle(),
            });
            host = Some(h.clone());
            gpui_kit::base::Root::new(h, window, cx)
        });
        let host = host.unwrap();
        cx.update(|window, cx| host.read(cx).focus.clone().focus(window, cx));
        cx.run_until_parked();
        (host, cx)
    }

    fn tab_focused(host: &Entity<Host>, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| {
            let ws = host.read(cx).ws.read(cx);
            let tab = ws.tabs().active().and_then(|t| t.item.focus_handle(cx));
            tab.is_some_and(|f| f.is_focused(window))
        })
    }

    /// Return on a file (quick open's own open leaves focus where it was) or a change: the
    /// keyboard is in what opened, so PgDn scrolls it with no ⌘E first.
    #[gpui_kit::test]
    fn a_file_or_change_picked_takes_the_keyboard(cx: &mut TestAppContext) {
        let (host, cx) = window("focus", cx);
        cx.update(|_, cx| {
            for kind in ["file", "unstaged"] {
                register_opener(cx, kind, |_, _, cx| {
                    Some(TabHandle::new(cx.new(|cx| Tab {
                        focus: cx.focus_handle(),
                    })))
                });
            }
            register_quick_open(
                cx,
                Rc::new(|ctx: &PanelContext, _: &mut App| {
                    let ws = ctx.workspace.clone();
                    let run: RunItem = Rc::new(move |window, cx| {
                        let file = Selection::File {
                            path: "src/a.rs".into(),
                        };
                        if let Some(ws) = ws.upgrade() {
                            ws.update(cx, |ws, cx| ws.open(file, false, window, cx));
                        }
                    });
                    vec![QuickItem {
                        label: "src/a.rs".into(),
                        detail: None,
                        is_path: true,
                        run,
                    }]
                }),
            );
        });
        let ws = cx.update(|_, cx| host.read(cx).ws.clone());
        cx.update(|window, cx| Palette::show(Mode::Files, &ws, window, cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert!(tab_focused(&host, cx), "the file's tab has the keyboard");

        // Back on the root, then a change picked from Open Changed File.
        cx.update(|window, cx| host.read(cx).focus.clone().focus(window, cx));
        let palette = cx
            .update(|window, cx| Palette::show(Mode::Changes, &ws, window, cx))
            .unwrap();
        cx.update(|_, cx| {
            palette.update(cx, |p, cx| {
                let file = Arc::new(FileChange {
                    path: "src/b.rs".into(),
                    old_path: None,
                    status: "M".into(),
                    additions: None,
                    deletions: None,
                    oid: None,
                    index_oid: None,
                    conflict: None,
                    mode: None,
                    submodule: None,
                    nested: None,
                });
                p.changes = Some(vec![(Selection::Unstaged { file: file.clone() }, file, "")]);
                p.rank(cx);
                cx.notify();
            })
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| ws.read(cx).tabs().len()), 2);
        assert!(tab_focused(&host, cx), "the change's tab has the keyboard");
    }

    /// A committed repository holding `a.rs` and `b.md`, opened in `host`'s workspace with the
    /// files crate's quick open; nothing has listed its files yet.
    fn open_repo(name: &str, host: &Entity<Host>, cx: &mut VisualTestContext) -> PathBuf {
        let repo = std::env::temp_dir().join(format!(
            "gitviber-palette-repo-{name}-{}",
            std::process::id()
        ));
        _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("a.rs"), "fn main() {}\n").unwrap();
        std::fs::write(repo.join("b.md"), "# b\n").unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap();
        };
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "init",
        ]);
        cx.update(|_, cx| {
            gitviber_ui::install_core(cx);
            gitviber_files::init(cx);
        });
        let ws = cx.update(|_, cx| host.read(cx).ws.clone());
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.open_project(repo.clone(), window, cx)));
        cx.run_until_parked();
        repo
    }

    /// ⌘P before quick open's first listing: open at once and quiet, then the files show up,
    /// ranked by what was typed meanwhile.
    #[gpui_kit::test]
    fn files_listed_after_the_palette_opened_show_up(cx: &mut TestAppContext) {
        let (host, cx) = window("listing", cx);
        let repo = open_repo("listing", &host, cx);
        let ws = cx.update(|_, cx| host.read(cx).ws.clone());
        let palette = cx
            .update(|window, cx| Palette::show(Mode::Files, &ws, window, cx))
            .unwrap();
        let rows = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                let p = palette.read(cx);
                let rows: Vec<String> = p.rows.iter().map(|r| r.label.to_string()).collect();
                (rows, p.empty_text(cx))
            })
        };
        assert_eq!(rows(cx), (vec![], "Listing files…".into()));
        cx.update(|_, cx| {
            palette.update(cx, |p, cx| {
                p.query = "a".into();
                p.rank(cx);
            })
        });
        cx.run_until_parked();
        assert_eq!(rows(cx).0, ["a.rs"]);
        _ = std::fs::remove_dir_all(&repo);
    }

    /// Quick open finding no files in an open repository says why, in git's words (an index
    /// git can't read here; an old git, the launch check's).
    #[gpui_kit::test]
    fn no_files_listed_shows_the_reason(cx: &mut TestAppContext) {
        let (host, cx) = window("unlisted", cx);
        let repo = open_repo("unlisted", &host, cx);
        std::fs::write(repo.join(".git/index"), "not an index").unwrap();
        let ws = cx.update(|_, cx| host.read(cx).ws.clone());
        let palette = cx
            .update(|window, cx| Palette::show(Mode::Files, &ws, window, cx))
            .unwrap();
        cx.run_until_parked();
        let shown = cx.update(|_, cx| palette.read(cx).empty_text(cx));
        assert!(shown.starts_with("Could not list files: "), "{shown}");
        assert!(shown.contains("index"), "{shown}");
        _ = std::fs::remove_dir_all(&repo);
    }
}
