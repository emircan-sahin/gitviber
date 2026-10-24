//! One terminal pane: its session, keys, text input, mouse, selection, find and cursor blink.
//! Painting is `element::TerminalElement`'s.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::search::Match;
use gitviber_core::{Core, api::app as pty, clipboard::Paste};
use gpui::{
    App, AppContext as _, Bounds, ClipboardItem, Context, Entity, EntityInputHandler, EventEmitter,
    ExternalPaths, FocusHandle, Focusable, InteractiveElement as _, IntoElement, KeyDownEvent,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    PinchEvent, Pixels, Point, PromptLevel, Render, ScrollDelta, ScrollWheelEvent, Size,
    Styled as _, Subscription, Task, UTF16Selection, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_kit::component::menu::ContextMenuExt as _;

use crate::config::{OptionAsMeta, TerminalConfig, TerminalTheme, to_rgb};
use crate::element::{RowCache, TerminalElement};
use crate::find::{self, FindOptions};
use crate::input::{self, GridSize, plan_fit};
use crate::keys;
use crate::layout::PaneId;
use crate::marks::{CommandEnd, end_title, strip_marks};
use crate::mouse::{self, Button, Kind, Mods};
use crate::session::{Note, Session, Spawn, term_config};
use crate::{
    Copy, CopyLastOutput, Paste as PasteAction, ScrollPageDown, ScrollPageUp, ScrollToBottom,
    ScrollToTop, SelectAll, SelectLastOutput,
};
use gitviber_ui::actions::terminal::{Clear as ClearTerminal, NextCommand, PrevCommand};

/// How long a pane's size holds still before its program hears of it: a divider drag changes the
/// rows each frame, and Claude Code redrew on each SIGWINCH, leaving copies of its screen in the
/// history.
const PTY_RESIZE_WAIT: Duration = Duration::from_millis(50);
/// Columns held back from a long history come due after this.
const HOLD_COLS: Duration = Duration::from_millis(100);
const BLINK: Duration = Duration::from_millis(530);
/// Blinking stops after this long without keys or output, so an idle pane wakes nothing.
const BLINK_IDLE: Duration = Duration::from_secs(30);
/// A shell gone sooner (a broken rc file) leaves its pane up to be read.
const QUICK_EXIT: Duration = Duration::from_secs(1);
/// How long a command waits for the first prompt: past it, typing ahead is no worse.
const RUN_WAIT: Duration = Duration::from_secs(5);
/// Find refreshes its matches at most this often while output arrives.
const FIND_REFRESH: Duration = Duration::from_millis(250);
/// Pinch zoom per font step.
const PINCH_STEP: f32 = 0.08;

/// The pane's padding: the left one holds the command marks.
pub(crate) const PAD_LEFT: f32 = 12.;
pub(crate) const PAD_TOP: f32 = 8.;
pub(crate) const PAD_RIGHT: f32 = 10.;
pub(crate) const PAD_BOTTOM: f32 = 4.;

#[derive(Clone, Debug, PartialEq)]
pub enum PaneEvent {
    TitleChanged,
    /// The shell ended. `quick`: within a second of starting, and its pane stays up to be read.
    Exited {
        quick: bool,
    },
    /// The pane took the keys.
    Focused,
    Bell,
    /// A marked command ended: where long-command notifications hook in.
    CommandEnded(CommandEnd),
    /// The shell prompted: a `cd` has settled, and an agent that ran there is gone.
    Prompt,
    /// A pinch over it asked the font a step bigger (1) or smaller (-1).
    FontStep(i32),
    FindChanged,
}

/// Where the grid is on screen, from the last prepaint.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Metrics {
    pub bounds: Bounds<Pixels>,
    pub origin: Point<Pixels>,
    pub cell: Size<Pixels>,
    pub cols: usize,
    pub rows: usize,
}

enum Drag {
    /// The program reads the mouse: the press went to it, so does the rest.
    Report {
        button: Button,
        last: (usize, usize),
    },
    Select {
        ty: SelectionType,
        anchor: (GridPoint, Side),
        moved: bool,
    },
}

/// What find shows for the pane it searches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindResult {
    /// 1-based; 0 when none is current.
    pub index: usize,
    pub total: usize,
    /// More matched than are marked.
    pub more: bool,
}

pub(crate) struct FindState {
    query: String,
    options: FindOptions,
    regex: Option<alacritty_terminal::term::search::RegexSearch>,
    pub matches: Vec<Match>,
    pub current: Option<usize>,
    more: bool,
    stale: bool,
}

pub struct TerminalPane {
    pub(crate) id: PaneId,
    pub(crate) session: Session,
    focus: FocusHandle,
    pub(crate) config: TerminalConfig,
    pub(crate) theme: TerminalTheme,
    /// Where it opened, which decides the worktree it belongs to; `dir` is where its shell went.
    cwd: PathBuf,
    dir: PathBuf,
    folder: Option<PathBuf>,
    title: Option<String>,
    started: bool,
    exited: bool,
    integrated: bool,
    /// Typed into the shell at its first prompt.
    run: Option<String>,
    pub(crate) metrics: Option<Metrics>,
    size: GridSize,
    proposed: Option<GridSize>,
    pty_size: Option<GridSize>,
    holding: bool,
    hold_task: Option<Task<()>>,
    resize_task: Option<Task<()>>,
    pub(crate) ime: Option<String>,
    drag: Option<Drag>,
    wheel_pending: f32,
    pinch: f32,
    last_hover_cell: Option<(usize, usize)>,
    pub(crate) focused: bool,
    window_active: bool,
    pub(crate) blink_visible: bool,
    blink_task: Option<Task<()>>,
    active_at: Instant,
    pub(crate) find: Option<FindState>,
    find_task: Option<Task<()>>,
    pub(crate) cache: Rc<RefCell<RowCache>>,
    pub(crate) hovered_mark: Option<(u32, Bounds<Pixels>)>,
    pub(crate) dim: f32,
    _wake: Task<()>,
    sync_task: Option<Task<()>>,
    run_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
    _live: gitviber_ui::memory::Live,
}

impl EventEmitter<PaneEvent> for TerminalPane {}

impl Focusable for TerminalPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(target_os = "macos")]
fn left_option_down() -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state: i32) -> u64;
    }
    // kCGEventSourceStateHIDSystemState; NX_DEVICELALTKEYMASK.
    // SAFETY: a pure query of the current modifier flags.
    unsafe { CGEventSourceFlagsState(1) & 0x20 != 0 }
}

#[cfg(not(target_os = "macos"))]
fn left_option_down() -> bool {
    false
}

impl TerminalPane {
    /// A pane whose shell starts in `folder` (or `cwd`) once it's laid out; `run` is typed at its
    /// first prompt.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        id: PaneId,
        core: Arc<Core>,
        cwd: PathBuf,
        folder: Option<PathBuf>,
        run: Option<String>,
        config: TerminalConfig,
        theme: TerminalTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let session = Session::new(
            core,
            term_config(config.scrollback, config.cursor, config.cursor_blink),
        );
        let outbox = session.outbox.clone();
        let wake = cx.spawn(async move |this, cx| {
            while outbox.signal.wait().await {
                if this.update(cx, |pane, cx| pane.on_wake(cx)).is_err() {
                    break;
                }
            }
        });
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus(&focus, window, |this, window, cx| {
                this.on_focus_change(true, window, cx)
            }),
            cx.on_blur(&focus, window, |this, window, cx| {
                this.on_focus_change(false, window, cx)
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                this.window_active = window.is_window_active();
                this.report_focus(cx);
                this.restart_blink(cx);
            }),
        ];
        let pane = TerminalPane {
            id,
            session,
            focus,
            dir: folder.clone().unwrap_or_else(|| cwd.clone()),
            cwd,
            folder,
            title: None,
            started: false,
            exited: false,
            integrated: false,
            run,
            metrics: None,
            size: GridSize { cols: 80, rows: 24 },
            proposed: None,
            pty_size: None,
            holding: false,
            hold_task: None,
            resize_task: None,
            ime: None,
            drag: None,
            wheel_pending: 0.,
            pinch: 0.,
            last_hover_cell: None,
            focused: false,
            window_active: window.is_window_active(),
            blink_visible: true,
            blink_task: None,
            active_at: Instant::now(),
            find: None,
            find_task: None,
            cache: Rc::default(),
            hovered_mark: None,
            dim: 0.,
            _wake: wake,
            sync_task: None,
            run_task: None,
            _subscriptions: subscriptions,
            _live: gitviber_ui::memory::Live::new("TerminalPane"),
            config: config.clone(),
            theme,
        };
        pane.apply_theme_answers();
        pane
    }

    pub fn id(&self) -> PaneId {
        self.id
    }

    /// Where it opened: the worktree (or folder) it belongs to.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Where the shell was last seen.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// What the shell or program set as the window title (OSC 0/2).
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn has_exited(&self) -> bool {
        self.exited
    }

    /// The session's pty id once the shell is up (GITVIBER_PTY in it).
    pub fn pty_id(&self) -> Option<u32> {
        self.session.pty()
    }

    /// A row's text, `line` 0 the top of the screen and negative into the history; None past the
    /// grid. The seam links and agent detection read rows through.
    pub fn row_text(&self, line: i32) -> Option<String> {
        let emu = self.session.emu.lock();
        let term = &emu.term;
        if line < term.topmost_line().0 || line > term.bottommost_line().0 {
            return None;
        }
        let text = term.bounds_to_string(
            GridPoint::new(Line(line), Column(0)),
            GridPoint::new(Line(line), term.last_column()),
        );
        Some(strip_marks(text).trim_end_matches('\n').to_string())
    }

    /// The rows on screen now, as `row_text` numbers them.
    pub fn visible_lines(&self) -> std::ops::Range<i32> {
        let emu = self.session.emu.lock();
        let top = -(emu.term.grid().display_offset() as i32);
        top..top + emu.term.screen_lines() as i32
    }

    pub(crate) fn set_config(&mut self, config: TerminalConfig, cx: &mut Context<Self>) {
        if (config.scrollback, config.cursor, config.cursor_blink)
            != (
                self.config.scrollback,
                self.config.cursor,
                self.config.cursor_blink,
            )
        {
            self.session.emu.lock().term.set_options(term_config(
                config.scrollback,
                config.cursor,
                config.cursor_blink,
            ));
        }
        if config.font_family != self.config.font_family
            || config.font_size != self.config.font_size
            || config.line_height != self.config.line_height
            || config.font_weight != self.config.font_weight
        {
            self.cache.borrow_mut().clear();
        }
        self.config = config;
        self.restart_blink(cx);
        cx.notify();
    }

    pub(crate) fn set_theme(&mut self, theme: TerminalTheme, cx: &mut Context<Self>) {
        if theme != self.theme {
            self.theme = theme;
            self.cache.borrow_mut().clear();
            self.apply_theme_answers();
            cx.notify();
        }
    }

    fn apply_theme_answers(&self) {
        let t = &self.theme;
        self.session.outbox.set_answers(|a| {
            a.foreground = to_rgb(t.foreground);
            a.background = to_rgb(t.background);
            a.cursor = to_rgb(t.cursor);
            for (i, c) in t.ansi.iter().enumerate() {
                a.ansi[i] = to_rgb(*c);
            }
        });
    }

    pub(crate) fn focus_handle_ref(&self) -> &FocusHandle {
        &self.focus
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
    }

    fn on_focus_change(&mut self, focused: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.focused = focused;
        if focused {
            cx.emit(PaneEvent::Focused);
        } else {
            self.ime = None;
        }
        self.report_focus(cx);
        self.restart_blink(cx);
        cx.notify();
    }

    /// Focus reports (1004): Claude Code listens for the pane, or the window, losing the keys.
    fn report_focus(&self, _: &mut Context<Self>) {
        if self
            .session
            .emu
            .lock()
            .mode()
            .contains(TermMode::FOCUS_IN_OUT)
        {
            self.session.write(if self.focused && self.window_active {
                "\x1b[I"
            } else {
                "\x1b[O"
            });
        }
    }

    /// Blinks only while it has the keys in the active window, and not for long idle.
    fn restart_blink(&mut self, cx: &mut Context<Self>) {
        self.blink_visible = true;
        self.active_at = Instant::now();
        if !(self.config.cursor_blink && self.focused && self.window_active) {
            self.blink_task = None;
            return;
        }
        if self.blink_task.is_some() {
            return;
        }
        self.blink_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                let go_on = this.update(cx, |pane, cx| {
                    if pane.active_at.elapsed() > BLINK_IDLE || !pane.focused || !pane.window_active
                    {
                        pane.blink_visible = true;
                        pane.blink_task = None;
                        cx.notify();
                        return false;
                    }
                    pane.blink_visible = !pane.blink_visible;
                    cx.notify();
                    true
                });
                if !go_on.unwrap_or(false) {
                    break;
                }
            }
        }));
    }

    fn typed(&mut self, cx: &mut Context<Self>) {
        self.active_at = Instant::now();
        if !self.blink_visible {
            self.blink_visible = true;
        }
        if self.blink_task.is_none() {
            self.restart_blink(cx);
        }
        // Typing goes back to the prompt, as other terminals do.
        let mut emu = self.session.emu.lock();
        if emu.term.grid().display_offset() != 0 {
            emu.term.scroll_display(Scroll::Bottom);
        }
    }

    fn on_wake(&mut self, cx: &mut Context<Self>) {
        for note in self.session.outbox.take_notes() {
            match note {
                Note::Started { integrated } => {
                    self.integrated = integrated;
                    self.type_run(false, cx);
                }
                Note::Failed => {}
                Note::Title(title) => {
                    self.title = title.filter(|t| !t.trim().is_empty());
                    cx.emit(PaneEvent::TitleChanged);
                }
                Note::Bell => cx.emit(PaneEvent::Bell),
                // Taken from the pane in use alone, where a yank or a tmux copy happens: output in
                // the background can't replace the clipboard.
                Note::Copy(text) => {
                    if self.focused && self.window_active {
                        let _ = pty::terminal_copy(self.session.core(), text);
                    }
                }
                Note::Prompt => {
                    if let Some(dir) = self.session.cwd() {
                        self.dir = dir.into();
                    }
                    self.type_run(true, cx);
                    cx.emit(PaneEvent::Prompt);
                }
                Note::Ended(end) => cx.emit(PaneEvent::CommandEnded(end)),
                Note::Cwd(dir) => self.dir = dir.into(),
                Note::Exited { exit, lived } => {
                    self.exited = true;
                    let quick = lived < QUICK_EXIT;
                    if quick {
                        let how = match exit {
                            Some(e) if e.signal.is_some() => {
                                format!(": {}", e.signal.unwrap_or_default())
                            }
                            Some(e) if e.code.is_some() => {
                                format!(" with code {}", e.code.unwrap_or_default())
                            }
                            _ => String::new(),
                        };
                        self.session
                            .emu
                            .lock()
                            .feed(format!("\r\n\x1b[2m[shell exited{how}]\x1b[0m\r\n").as_bytes());
                    }
                    cx.emit(PaneEvent::Exited { quick });
                }
            }
        }
        if let Some(deadline) = self.session.outbox.take_sync_deadline() {
            self.sync_task = Some(cx.spawn(async move |this, cx| {
                let wait = deadline.saturating_duration_since(Instant::now());
                cx.background_executor()
                    .timer(wait + Duration::from_millis(1))
                    .await;
                let _ = this.update(cx, |pane, cx| {
                    let mut emu = pane.session.emu.lock();
                    let emu = &mut *emu;
                    // A frame never ended (BSU without ESU): show what came.
                    if emu
                        .parser
                        .sync_timeout()
                        .sync_timeout()
                        .is_some_and(|t| t <= Instant::now())
                    {
                        emu.parser.stop_sync(&mut emu.term);
                    }
                    cx.notify();
                });
            }));
        }
        if let Some(find) = self.find.as_mut() {
            find.stale = true;
            if self.find_task.is_none() {
                self.find_task = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(FIND_REFRESH).await;
                    let _ = this.update(cx, |pane, cx| {
                        pane.find_task = None;
                        pane.refresh_find(cx);
                    });
                }));
            }
        }
        cx.notify();
    }

    /// The command for the first prompt: at it where the integration says when that is (an rc
    /// file that reads the terminal or clears it can't take it), else typed ahead.
    fn type_run(&mut self, at_prompt: bool, cx: &mut Context<Self>) {
        if self.run.is_none() {
            return;
        }
        if !at_prompt {
            if !self.integrated {
                let run = self.run.take().unwrap_or_default();
                self.session.write(run);
            } else {
                self.run_task = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(RUN_WAIT).await;
                    let _ = this.update(cx, |pane, cx| pane.type_run(true, cx));
                }));
            }
            return;
        }
        // Whatever was typed before the prompt is on its line: ⌃U clears it first, where ⌃C
        // would draw a new prompt.
        let run = self.run.take().unwrap_or_default();
        self.session.write(format!("\x15{run}"));
        self.run_task = None;
    }

    /// Fits the grid to the room the element measured; starts the shell at its first size.
    pub(crate) fn fit(&mut self, metrics: Metrics, cx: &mut Context<Self>) {
        let first_metrics = self.metrics.is_none_or(|m| {
            m.cell != metrics.cell || m.cols != metrics.cols || m.rows != metrics.rows
        });
        self.metrics = Some(metrics);
        let proposed = GridSize {
            cols: metrics.cols.min(u16::MAX as usize) as u16,
            rows: metrics.rows.min(u16::MAX as usize) as u16,
        };
        self.proposed = Some(proposed);
        if first_metrics {
            let (w, h) = (
                f32::from(metrics.cell.width),
                f32::from(metrics.cell.height),
            );
            self.session.outbox.set_answers(|a| {
                a.size = alacritty_terminal::event::WindowSize {
                    num_lines: proposed.rows,
                    num_cols: proposed.cols,
                    cell_width: w.round() as u16,
                    cell_height: h.round() as u16,
                };
            });
        }
        let history = if self.started {
            self.session.emu.lock().term.history_size()
        } else {
            0
        };
        let (w, h) = (
            f32::from(metrics.bounds.size.width),
            f32::from(metrics.bounds.size.height),
        );
        self.apply_fit(w, h, history, cx);
        if !self.started {
            self.started = true;
            self.session.spawn(Spawn {
                cwd: self.cwd.to_string_lossy().into_owned(),
                folder: self
                    .folder
                    .as_ref()
                    .map(|f| f.to_string_lossy().into_owned()),
                cols: self.size.cols,
                rows: self.size.rows,
                integration: self.config.shell_integration,
            });
            self.pty_size = Some(self.size);
        }
    }

    fn apply_fit(&mut self, width: f32, height: f32, history: usize, cx: &mut Context<Self>) {
        let Some((size, cols_later)) = plan_fit(width, height, self.proposed, self.size, history)
        else {
            return;
        };
        let was_holding = self.holding;
        self.holding = cols_later;
        if cols_later {
            if self.hold_task.is_none() {
                self.hold_task = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(HOLD_COLS).await;
                    let _ = this.update(cx, |pane, cx| {
                        pane.hold_task = None;
                        if let Some(m) = pane.metrics {
                            let (w, h) = (
                                f32::from(m.bounds.size.width),
                                f32::from(m.bounds.size.height),
                            );
                            pane.apply_fit(w, h, 0, cx);
                            cx.notify();
                        }
                    });
                }));
            }
        } else {
            self.hold_task = None;
        }
        if size != self.size {
            self.session
                .emu
                .lock()
                .term
                .resize(alacritty_terminal::term::test::TermSize::new(
                    size.cols as usize,
                    size.rows as usize,
                ));
            self.size = size;
            self.cache.borrow_mut().clear();
        }
        if self.started && (size != self.pty_size.unwrap_or(size) || was_holding != cols_later) {
            self.resize_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(PTY_RESIZE_WAIT).await;
                let _ = this.update(cx, |pane, _| pane.send_size());
            }));
        }
    }

    /// Tells the program its size once a resize settled, held columns included: one SIGWINCH.
    fn send_size(&mut self) {
        if self.holding || Some(self.size) == self.pty_size {
            return;
        }
        self.pty_size = Some(self.size);
        self.session.resize_pty(self.size.cols, self.size.rows);
    }

    pub(crate) fn mode(&self) -> TermMode {
        self.session.emu.lock().mode()
    }

    fn alt_is_meta(&self) -> bool {
        match self.config.option_as_meta {
            OptionAsMeta::Off => false,
            OptionAsMeta::Both => true,
            OptionAsMeta::Left => left_option_down(),
        }
    }

    fn key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.ime.is_some() || self.exited {
            return;
        }
        let k = &e.keystroke;
        let m = k.modifiers;
        let mode = self.mode();
        // Linux terminals copy and paste with Ctrl+Shift+C/V: Ctrl+C and Ctrl+V are the shell's.
        if !cfg!(target_os = "macos") && m.control && m.shift && !m.alt && !m.platform {
            match k.key.as_str() {
                "c" => return self.copy_selection(cx, true),
                "v" => {
                    cx.stop_propagation();
                    return self.paste(window, cx);
                }
                _ => {}
            }
        }
        if !cfg!(target_os = "macos") && m.shift && !m.control && !m.alt && k.key == "insert" {
            cx.stop_propagation();
            return self.paste(window, cx);
        }
        // Shift+PgUp/PgDn page the history; a full-screen program keeps them.
        if m.shift && !m.control && !m.alt && !m.platform && !mode.contains(TermMode::ALT_SCREEN) {
            let scroll = match k.key.as_str() {
                "pageup" => Some(Scroll::PageUp),
                "pagedown" => Some(Scroll::PageDown),
                _ => None,
            };
            if let Some(s) = scroll {
                self.session.emu.lock().term.scroll_display(s);
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }
        let key = keys::Key {
            key: &k.key,
            ctrl: m.control,
            alt: m.alt,
            shift: m.shift,
            cmd: m.platform,
        };
        let modes = keys::Modes {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            kitty: mode.contains(TermMode::DISAMBIGUATE_ESC_CODES),
            kitty_all: mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC),
            alt_is_meta: m.alt && self.alt_is_meta(),
            mac: cfg!(target_os = "macos"),
        };
        match keys::encode(&key, &modes) {
            keys::Action::Send(bytes) => {
                self.session.write(bytes);
                self.typed(cx);
                cx.stop_propagation();
                cx.notify();
            }
            keys::Action::Text | keys::Action::Pass => {}
        }
    }

    fn copy_selection(&mut self, cx: &mut Context<Self>, stop: bool) {
        let text = self.session.emu.lock().term.selection_to_string();
        match text.map(strip_marks).filter(|t| !t.is_empty()) {
            Some(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                if stop {
                    cx.stop_propagation();
                }
            }
            None => cx.propagate(),
        }
    }

    fn on_copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_selection(cx, false);
    }

    fn on_paste(&mut self, _: &PasteAction, window: &mut Window, cx: &mut Context<Self>) {
        self.paste(window, cx);
    }

    /// The native clipboard (core's): a copied image or Finder file pastes its path.
    pub fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.exited {
            return;
        }
        let got = pty::terminal_paste(self.session.core());
        let got = match got {
            Ok(Paste::Empty) | Err(_) => match cx.read_from_clipboard().and_then(|c| c.text()) {
                Some(text) => Paste::Text { text },
                None => return,
            },
            Ok(p) => p,
        };
        match got {
            Paste::Text { text } => self.paste_text(text, window, cx),
            Paste::Files { paths } => self.paste_paths(&paths, cx),
            Paste::Image { path } => self.paste_paths(&[path], cx),
            Paste::Empty => {}
        }
    }

    /// Several lines into a program without bracketed paste run one by one as they arrive: asked
    /// first.
    pub fn paste_text(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.exited || text.is_empty() {
            return;
        }
        let bracketed = self.mode().contains(TermMode::BRACKETED_PASTE);
        let lines = input::pasted_lines(&text);
        if lines > 1 && !bracketed {
            let answer = window.prompt(
                PromptLevel::Warning,
                &format!("Paste {lines} lines"),
                Some(&format!(
                    "The program in this terminal takes a paste as typed keys, so each of the {lines} lines runs as it arrives."
                )),
                &["Paste", "Cancel"],
                cx,
            );
            cx.spawn(async move |this, cx| {
                if answer.await.ok() == Some(0) {
                    let _ = this.update(cx, |pane, cx| {
                        let bracketed = pane.mode().contains(TermMode::BRACKETED_PASTE);
                        pane.session.write(input::paste_bytes(&text, bracketed));
                        pane.typed(cx);
                        cx.notify();
                    });
                }
            })
            .detach();
            return;
        }
        self.session.write(input::paste_bytes(&text, bracketed));
        self.typed(cx);
        cx.notify();
    }

    /// Paths as the AI CLIs take them: each its own (bracketed) paste, never typed as keys.
    pub fn paste_paths(&mut self, paths: &[String], cx: &mut Context<Self>) {
        if self.exited {
            return;
        }
        let bracketed = self.mode().contains(TermMode::BRACKETED_PASTE);
        for text in input::path_pastes(paths) {
            self.session.write(input::paste_bytes(&text, bracketed));
        }
        self.typed(cx);
        cx.notify();
    }

    fn on_drop(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<String> = paths
            .paths()
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let kept = pty::keep_dropped(self.session.core(), paths);
        self.paste_paths(&kept, cx);
        self.focus(window, cx);
    }

    fn on_select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let mut emu = self.session.emu.lock();
        let term = &mut emu.term;
        let mut s = Selection::new(
            SelectionType::Lines,
            GridPoint::new(term.topmost_line(), Column(0)),
            Side::Left,
        );
        s.update(
            GridPoint::new(term.bottommost_line(), term.last_column()),
            Side::Right,
        );
        term.selection = Some(s);
        drop(emu);
        cx.notify();
    }

    /// Clears the history and the screen above the prompt. A running program (Claude Code, vim)
    /// is left alone: it redraws relative to the rows it drew.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.mode().contains(TermMode::ALT_SCREEN) || self.session.busy() {
            return;
        }
        let mut emu = self.session.emu.lock();
        let line = emu.term.grid().cursor.point.line.0.max(0);
        let seq = if line > 0 {
            format!("\x1b[{line}S\x1b[{line}A\x1b[3J")
        } else {
            "\x1b[3J".into()
        };
        emu.feed(seq.as_bytes());
        emu.term.selection = None;
        drop(emu);
        self.cache.borrow_mut().clear();
        cx.notify();
    }

    fn on_clear(&mut self, _: &ClearTerminal, _: &mut Window, cx: &mut Context<Self>) {
        self.clear(cx);
    }

    /// ⌘↑ / ⌘↓ between the marked prompts; a full-screen program's, or a shell's with no marks,
    /// keep the keys.
    fn jump(&mut self, dir: i32, cx: &mut Context<Self>) {
        let mut emu = self.session.emu.lock();
        let emu = &mut *emu;
        if emu.term.mode().contains(TermMode::ALT_SCREEN) || !emu.marks.has_commands() {
            cx.propagate();
            return;
        }
        emu.marks.jump(&mut emu.term, dir);
        cx.notify();
    }

    fn scroll_history(&mut self, scroll: Scroll, cx: &mut Context<Self>) {
        let mut emu = self.session.emu.lock();
        if emu.term.mode().contains(TermMode::ALT_SCREEN) {
            cx.propagate();
            return;
        }
        emu.term.scroll_display(scroll);
        cx.notify();
    }

    /// The last command's output, from the integration's marks; None without one.
    pub fn last_output(&self) -> Option<String> {
        let emu = self.session.emu.lock();
        emu.marks.last_output(&emu.term)
    }

    pub fn has_last_output(&self) -> bool {
        let emu = self.session.emu.lock();
        emu.marks.has_output(&emu.term)
    }

    pub fn has_selection(&self) -> bool {
        self.session
            .emu
            .lock()
            .term
            .selection
            .as_ref()
            .is_some_and(|s| !s.is_empty())
    }

    pub fn copy_last_output(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.last_output().filter(|t| !t.is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub fn select_last_output(&mut self, cx: &mut Context<Self>) {
        let mut emu = self.session.emu.lock();
        let emu = &mut *emu;
        emu.marks.select_last_output(&mut emu.term);
        cx.notify();
    }

    /// A program reading the mouse gets clicks, unless ⌥ (⇧ off macOS) forces a selection past it.
    fn program_takes_mouse(&self, m: &Modifiers, mode: TermMode) -> bool {
        let force = if cfg!(target_os = "macos") {
            m.alt
        } else {
            m.shift
        };
        mode.intersects(TermMode::MOUSE_MODE) && !force
    }

    /// The cell under `pos`, its side, and whether it's inside the grid.
    fn cell_at(&self, pos: Point<Pixels>) -> Option<(usize, usize, Side, bool)> {
        let m = self.metrics?;
        let (cw, ch) = (f32::from(m.cell.width), f32::from(m.cell.height));
        let x = f32::from(pos.x - m.origin.x) / cw;
        let y = f32::from(pos.y - m.origin.y) / ch;
        let inside = x >= 0. && y >= 0. && (x as usize) < m.cols && (y as usize) < m.rows;
        let col = (x.max(0.) as usize).min(m.cols.saturating_sub(1));
        let row = (y.max(0.) as usize).min(m.rows.saturating_sub(1));
        let side = if x.fract() < 0.5 || x < 0. {
            Side::Left
        } else {
            Side::Right
        };
        Some((col, row, side, inside))
    }

    fn mods(m: &Modifiers) -> Mods {
        Mods {
            shift: m.shift,
            alt: m.alt,
            ctrl: m.control,
        }
    }

    fn send_report(
        &self,
        button: Button,
        kind: Kind,
        col: usize,
        row: usize,
        m: &Modifiers,
        mode: TermMode,
    ) {
        if let Some((data, binary)) = mouse::report(button, kind, col, row, Self::mods(m), mode) {
            if binary {
                self.session.write_binary(data);
            } else {
                self.session.write(data);
            }
        }
    }

    pub(crate) fn mouse_down(
        &mut self,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((col, row, side, _)) = self.cell_at(e.position) else {
            return;
        };
        self.focus(window, cx);
        let mode = self.mode();
        let button = match e.button {
            MouseButton::Left => Button::Left,
            MouseButton::Middle => Button::Middle,
            MouseButton::Right => Button::Right,
            _ => return,
        };
        if self.program_takes_mouse(&e.modifiers, mode) {
            self.send_report(button, Kind::Press, col, row, &e.modifiers, mode);
            self.drag = Some(Drag::Report {
                button,
                last: (col, row),
            });
            cx.stop_propagation();
            return;
        }
        if button != Button::Left {
            return;
        }
        let mut emu = self.session.emu.lock();
        let offset = emu.term.grid().display_offset() as i32;
        let point = GridPoint::new(Line(row as i32 - offset), Column(col));
        if e.modifiers.shift && e.click_count == 1 && emu.term.selection.is_some() {
            if let Some(s) = emu.term.selection.as_mut() {
                s.update(point, side);
            }
            drop(emu);
            cx.notify();
            return;
        }
        let ty = match e.click_count {
            2 => SelectionType::Semantic,
            n if n >= 3 => SelectionType::Lines,
            // ⌥-drag selects a block where no program reads the mouse.
            _ if cfg!(target_os = "macos")
                && e.modifiers.alt
                && !mode.intersects(TermMode::MOUSE_MODE) =>
            {
                SelectionType::Block
            }
            _ => SelectionType::Simple,
        };
        emu.term.selection = Some(Selection::new(ty, point, side));
        drop(emu);
        self.drag = Some(Drag::Select {
            ty,
            anchor: (point, side),
            moved: e.click_count > 1,
        });
        cx.notify();
    }

    pub(crate) fn mouse_move(
        &mut self,
        e: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((col, row, side, inside)) = self.cell_at(e.position) else {
            return;
        };
        let mode = self.mode();
        match self.drag.as_mut() {
            Some(Drag::Report { button, last }) => {
                // A program re-sending its mouse modes mid-drag (Claude Code does on resize and
                // focus) still gets the rest of it.
                let (button, changed) = (*button, *last != (col, row));
                *last = (col, row);
                if changed && mouse::wants(Kind::Motion, button, mode) {
                    self.send_report(button, Kind::Motion, col, row, &e.modifiers, mode);
                }
            }
            Some(Drag::Select { ty, anchor, moved }) => {
                *moved = true;
                let (ty, anchor) = (*ty, *anchor);
                let mut emu = self.session.emu.lock();
                // Past the top or bottom: the history scrolls under the drag.
                if let Some(m) = self.metrics {
                    let y = e.position.y;
                    if y < m.origin.y {
                        emu.term.scroll_display(Scroll::Delta(1));
                    } else if y > m.origin.y + m.cell.height * m.rows as f32 {
                        emu.term.scroll_display(Scroll::Delta(-1));
                    }
                }
                let offset = emu.term.grid().display_offset() as i32;
                let point = GridPoint::new(Line(row as i32 - offset), Column(col));
                // Output that cleared the selected rows dropped it; the drag goes on from its start.
                let s = emu
                    .term
                    .selection
                    .get_or_insert_with(|| Selection::new(ty, anchor.0, anchor.1));
                s.update(point, side);
                drop(emu);
                cx.notify();
            }
            None => {
                if inside
                    && mode.contains(TermMode::MOUSE_MOTION)
                    && self.last_hover_cell != Some((col, row))
                {
                    self.send_report(Button::None, Kind::Motion, col, row, &e.modifiers, mode);
                }
                self.last_hover_cell = inside.then_some((col, row));
                self.update_hovered_mark(e.position, cx);
            }
        }
    }

    fn update_hovered_mark(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let hovered = self.mark_at(pos);
        if hovered != self.hovered_mark {
            self.hovered_mark = hovered;
            cx.notify();
        }
    }

    /// The command mark in the left padding under `pos`.
    fn mark_at(&self, pos: Point<Pixels>) -> Option<(u32, Bounds<Pixels>)> {
        let m = self.metrics?;
        if pos.x > m.origin.x || pos.x < m.bounds.origin.x {
            return None;
        }
        let row = (f32::from(pos.y - m.origin.y) / f32::from(m.cell.height)).floor();
        if row < 0. || row as usize >= m.rows {
            return None;
        }
        let emu = self.session.emu.lock();
        let line = Line(row as i32 - emu.term.grid().display_offset() as i32);
        let id = crate::marks::mark_on(&emu.term, line)?;
        emu.marks.command(id)?.end.as_ref()?;
        let top = m.origin.y + m.cell.height * row;
        Some((
            id,
            Bounds::new(
                gpui::point(m.bounds.origin.x, top),
                gpui::size(px(PAD_LEFT), m.cell.height),
            ),
        ))
    }

    /// The hover over a command mark: how long it took and how it ended.
    pub(crate) fn mark_title(&self, id: u32) -> Option<String> {
        let emu = self.session.emu.lock();
        let ended = emu.marks.command(id)?.end.as_ref()?;
        Some(end_title(&ended.end))
    }

    pub(crate) fn mouse_up(&mut self, e: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        match drag {
            Drag::Report { button, .. } => {
                let Some((col, row, ..)) = self.cell_at(e.position) else {
                    return;
                };
                let mode = self.mode();
                if mode.intersects(TermMode::MOUSE_MODE) {
                    self.send_report(button, Kind::Release, col, row, &e.modifiers, mode);
                }
            }
            Drag::Select { moved, .. } => {
                let mut emu = self.session.emu.lock();
                // A click without a drag selects nothing.
                if !moved || emu.term.selection.as_ref().is_some_and(|s| s.is_empty()) {
                    emu.term.selection = None;
                }
                drop(emu);
                cx.notify();
            }
        }
    }

    pub(crate) fn scroll_wheel(
        &mut self,
        e: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(m) = self.metrics else { return };
        // Ctrl+wheel sizes the font, as a pinch does.
        if e.modifiers.control {
            let dy = match e.delta {
                ScrollDelta::Pixels(p) => f32::from(p.y),
                ScrollDelta::Lines(l) => l.y * 16.,
            };
            self.pinch += dy / 300.;
            self.pinch_step(cx);
            return;
        }
        let line_h = f32::from(m.cell.height);
        let mode = self.mode();
        let rows = match e.delta {
            // A swipe moves a row for each row's height of pixels, the rest kept for the next one.
            ScrollDelta::Pixels(p)
                if cfg!(target_os = "macos") || !mode.intersects(TermMode::MOUSE_MODE) =>
            {
                let (rows, pending) =
                    input::wheel_rows(self.wheel_pending, f32::from(p.y), line_h, m.rows as i32);
                self.wheel_pending = pending;
                rows
            }
            // A mouse wheel's notch arrives as 50-100 px off macOS: a report a notch.
            ScrollDelta::Pixels(p) => f32::from(p.y).signum() as i32,
            ScrollDelta::Lines(l) => {
                let n = l.y.round() as i32;
                if n == 0 && l.y != 0. {
                    l.y.signum() as i32
                } else {
                    n
                }
            }
        };
        if rows == 0 {
            return;
        }
        let up = rows > 0;
        let n = rows.unsigned_abs() as usize;
        if self.program_takes_mouse(&e.modifiers, mode) {
            // Claude Code's fullscreen view, vim, htop: a report for each row the swipe moved.
            if let Some((col, row, ..)) = self.cell_at(e.position) {
                let button = if up {
                    Button::WheelUp
                } else {
                    Button::WheelDown
                };
                for _ in 0..n {
                    self.send_report(button, Kind::Press, col, row, &e.modifiers, mode);
                }
            }
        } else if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
            let key = match (up, mode.contains(TermMode::APP_CURSOR)) {
                (true, false) => "\x1b[A",
                (false, false) => "\x1b[B",
                (true, true) => "\x1bOA",
                (false, true) => "\x1bOB",
            };
            self.session.write(key.repeat(n));
        } else {
            self.session
                .emu
                .lock()
                .term
                .scroll_display(Scroll::Delta(rows));
            cx.notify();
        }
        cx.stop_propagation();
    }

    pub(crate) fn pinched(&mut self, e: &PinchEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.pinch += e.delta;
        self.pinch_step(cx);
    }

    fn pinch_step(&mut self, cx: &mut Context<Self>) {
        if self.pinch.abs() >= PINCH_STEP {
            cx.emit(PaneEvent::FontStep(if self.pinch > 0. { 1 } else { -1 }));
            self.pinch = 0.;
        }
    }

    /// Searches the pane: `step` 0 as the query is typed (staying on the match while it still
    /// matches), 1 or -1 for the next or previous. An empty query clears it; a regex that doesn't
    /// parse is returned as the error.
    pub fn find(
        &mut self,
        query: &str,
        options: FindOptions,
        step: i32,
        cx: &mut Context<Self>,
    ) -> Result<FindResult, String> {
        if query.is_empty() {
            self.clear_find(cx);
            return Ok(FindResult::default());
        }
        let same = self
            .find
            .as_ref()
            .is_some_and(|f| f.query == query && f.options == options && !f.stale);
        if !same {
            let regex = find::compile(query, options).inspect_err(|_| self.clear_find(cx))?;
            let current = self
                .find
                .as_ref()
                .and_then(|f| f.current.and_then(|i| f.matches.get(i).cloned()));
            self.find = Some(FindState {
                query: query.into(),
                options,
                regex: Some(regex),
                matches: vec![],
                current: None,
                more: false,
                stale: true,
            });
            self.refresh_find(cx);
            // Stay on the match on show while it still matches.
            if let (Some(f), Some(was)) = (self.find.as_mut(), current) {
                f.current = f
                    .matches
                    .iter()
                    .position(|m| m.start() == was.start())
                    .or(f.current);
            }
        }
        let Some(f) = self.find.as_mut() else {
            return Ok(FindResult::default());
        };
        let n = f.matches.len();
        if n > 0 && step != 0 {
            f.current = Some(match f.current {
                Some(i) => (i as i64 + step as i64).rem_euclid(n as i64) as usize,
                None if step > 0 => 0,
                None => n - 1,
            });
        }
        let target = f.current.and_then(|i| f.matches.get(i)).map(|m| *m.start());
        if let Some(point) = target {
            self.session.emu.lock().term.scroll_to_point(point);
        }
        cx.notify();
        Ok(self.find_result())
    }

    pub fn find_result(&self) -> FindResult {
        match &self.find {
            Some(f) => FindResult {
                index: f.current.map_or(0, |i| i + 1),
                total: f.matches.len(),
                more: f.more,
            },
            None => FindResult::default(),
        }
    }

    fn refresh_find(&mut self, cx: &mut Context<Self>) {
        let Some(f) = self.find.as_mut() else { return };
        if !f.stale {
            return;
        }
        f.stale = false;
        let Some(regex) = f.regex.as_mut() else {
            return;
        };
        let emu = self.session.emu.lock();
        let was = f.current.and_then(|i| f.matches.get(i)).map(|m| *m.start());
        let (matches, more) = find::find_all(&emu.term, regex, f.options);
        let top = Line(-(emu.term.grid().display_offset() as i32));
        drop(emu);
        f.current = match was {
            Some(p) => matches
                .iter()
                .position(|m| *m.start() >= p)
                .or(matches.len().checked_sub(1)),
            // The first one from the top of the view down, else the last above it.
            None => matches
                .iter()
                .position(|m| m.start().line >= top)
                .or(matches.len().checked_sub(1)),
        };
        f.matches = matches;
        f.more = more;
        cx.emit(PaneEvent::FindChanged);
        cx.notify();
    }

    /// Find's marks go.
    pub fn clear_find(&mut self, cx: &mut Context<Self>) {
        if self.find.take().is_some() {
            self.find_task = None;
            cx.notify();
        }
    }

    fn context_menu_state(this: &gpui::WeakEntity<Self>, cx: &App) -> (bool, bool, bool) {
        this.upgrade()
            .map(|p| {
                let p = p.read(cx);
                (p.has_selection(), !p.exited, p.has_last_output())
            })
            .unwrap_or_default()
    }
}

impl Render for TerminalPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let primary = self.theme.primary;
        let weak = cx.entity().downgrade();
        let focus = self.focus.clone();
        let hovered = self
            .hovered_mark
            .and_then(|(id, b)| Some((self.mark_title(id)?, b)));
        let elevated = self.theme.elevated;
        let border = self.theme.border_strong;
        let fg = self.theme.foreground;
        div()
            .id(("terminal-pane", self.id))
            .track_focus(&self.focus)
            .key_context(crate::PANE_CONTEXT)
            .size_full()
            .relative()
            .opacity(1. - self.dim)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_paste))
            .on_action(cx.listener(Self::on_select_all))
            .on_action(cx.listener(Self::on_clear))
            .on_action(cx.listener(|this, _: &PrevCommand, _, cx| this.jump(-1, cx)))
            .on_action(cx.listener(|this, _: &NextCommand, _, cx| this.jump(1, cx)))
            .on_action(
                cx.listener(|this, _: &ScrollToTop, _, cx| this.scroll_history(Scroll::Top, cx)),
            )
            .on_action(cx.listener(|this, _: &ScrollToBottom, _, cx| {
                this.scroll_history(Scroll::Bottom, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ScrollPageUp, _, cx| {
                    this.scroll_history(Scroll::PageUp, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ScrollPageDown, _, cx| {
                this.scroll_history(Scroll::PageDown, cx)
            }))
            .on_action(cx.listener(|this, _: &CopyLastOutput, _, cx| this.copy_last_output(cx)))
            .on_action(cx.listener(|this, _: &SelectLastOutput, _, cx| this.select_last_output(cx)))
            .on_drop(cx.listener(Self::on_drop))
            .drag_over::<ExternalPaths>(move |style, _, _, _| {
                style
                    .border_2()
                    .border_color(primary)
                    .bg(primary.opacity(0.08))
            })
            .child(TerminalElement::new(cx.entity()))
            .when_some(hovered, |d, (title, b)| {
                let m = self.metrics;
                let left = m.map_or(px(0.), |m| b.origin.x - m.bounds.origin.x + px(PAD_LEFT));
                let top = m.map_or(px(0.), |m| b.origin.y - m.bounds.origin.y);
                d.child(
                    div()
                        .absolute()
                        .left(left)
                        .top(top - px(2.))
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .border_1()
                        .border_color(border)
                        .bg(elevated)
                        .text_color(fg)
                        .text_size(px(11.5))
                        .child(title),
                )
            })
            .context_menu(move |menu, _, cx| {
                let (selection, paste, output) = Self::context_menu_state(&weak, cx);
                menu.action_context(focus.clone())
                    .menu_with_disabled("Copy", Box::new(Copy), !selection)
                    .menu_with_disabled("Paste", Box::new(PasteAction), !paste)
                    .separator()
                    .menu_with_disabled(
                        "Copy Last Command Output",
                        Box::new(CopyLastOutput),
                        !output,
                    )
                    .menu_with_disabled(
                        "Select Last Command Output",
                        Box::new(SelectLastOutput),
                        !output,
                    )
            })
    }
}

/// Text input: what an IME composes shows at the cursor, what it commits (and plain typing) goes to
/// the program.
impl EntityInputHandler for TerminalPane {
    fn text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        adjusted: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let marked = self.ime.as_deref()?;
        let utf16: Vec<u16> = marked.encode_utf16().collect();
        let range = range.start.min(utf16.len())..range.end.min(utf16.len());
        *adjusted = Some(range.clone());
        String::from_utf16(&utf16[range]).ok()
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let len = self.ime.as_deref().map_or(0, |m| m.encode_utf16().count());
        Some(UTF16Selection {
            range: len..len,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        self.ime.as_deref().map(|m| 0..m.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.ime = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ime = None;
        if self.exited || text.is_empty() {
            cx.notify();
            return;
        }
        // Return and Tab arrive as text when an IME passes them on.
        let text = text.replace('\n', "\r");
        self.session.write(text);
        self.typed(cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        text: &str,
        _: Option<std::ops::Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ime = (!text.is_empty()).then(|| text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // The candidates window opens at the cursor.
        let m = self.metrics?;
        let emu = self.session.emu.lock();
        let cursor = emu.term.grid().cursor.point;
        let row = cursor.line.0 + emu.term.grid().display_offset() as i32;
        let origin = m.origin
            + gpui::point(
                m.cell.width * cursor.column.0 as f32,
                m.cell.height * row as f32,
            );
        Some(Bounds::new(origin, m.cell))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

/// A new pane entity, for the panel.
#[allow(clippy::too_many_arguments)]
pub(crate) fn new_pane(
    id: PaneId,
    core: Arc<Core>,
    cwd: PathBuf,
    folder: Option<PathBuf>,
    run: Option<String>,
    config: TerminalConfig,
    theme: TerminalTheme,
    window: &mut Window,
    cx: &mut App,
) -> Entity<TerminalPane> {
    cx.new(|cx| TerminalPane::new(id, core, cwd, folder, run, config, theme, window, cx))
}
