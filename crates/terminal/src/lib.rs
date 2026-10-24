//! GitViber's terminal: alacritty's emulation painted by a GPUI element, with tabs, splits and
//! zoom, a set of tabs per worktree, shell integration marks and find. Built for running Claude
//! Code and Codex in it.
//!
//! The app mounts one [`TerminalPanel`] and calls [`init`] once:
//!
//! ```ignore
//! gitviber_terminal::init(cx);
//! let panel = cx.new(|cx| TerminalPanel::new(core, root, config, theme, window, cx));
//! panel.update(cx, |p, cx| p.set_worktree(other_root, window, cx)); // swaps the tab set
//! ```
//!
//! - [`TerminalPanel`]: the bottom panel, its toolbar (new tab, split, kill, zoom, maximize,
//!   hide) and tabs; emits [`PanelEvent`]s (empty, maximize, hide, font size, a command's end).
//! - [`TerminalConfig`] / [`TerminalTheme`]: the Settings and theme inputs, filled from
//!   gitviber-ui by the app ([`TerminalPanel::set_config`], [`TerminalPanel::set_theme`]).
//! - [`TerminalPane`]: one pane, for what the later links and agent work hooks into:
//!   [`TerminalPane::row_text`], [`TerminalPane::visible_lines`], [`PaneEvent::CommandEnded`],
//!   [`PaneEvent::Prompt`], [`TerminalPane::pty_id`].

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

pub mod agent;
mod boxdraw;
mod config;
mod element;
mod find;
mod follow;
mod hints;
mod history;
mod input;
mod keys;
pub mod layout;
pub mod linkhost;
pub mod links;
mod marks;
mod mouse;
mod pane;
mod panel;
mod scan;
mod session;

pub use config::{
    CursorStyle, DEFAULT_FONT_SIZE, FONT_MAX, FONT_MIN, LINE_HEIGHTS, OptionAsMeta,
    SCROLLBACK_LINES, TerminalConfig, TerminalTheme, ansi_colors,
};
pub use find::FindOptions;
pub use marks::{CommandEnd, end_text, end_title, format_duration, short_command};
pub use pane::{FindResult, PaneEvent, TerminalPane};
pub use panel::{
    PanelEvent, TerminalPanel, WorktreeInfo, agents_in, calling, config_from, paste_to_agent,
    save_before_quit, stopped_by_quit, theme_from, worktree_agent,
};

use gpui::{App, KeyBinding, NoAction};

/// The pane's key context (gitviber-ui's TERMINAL_CONTEXT: the app's keys skip it), and the
/// panel's around it.
pub const PANE_CONTEXT: &str = gitviber_ui::keymap::TERMINAL_CONTEXT;
pub const PANEL_CONTEXT: &str = "TerminalPanel";

// The panel's commands (new, split, close, panes, zoom, maximize, rename, font, command jumps)
// are gitviber-ui's `terminal::*`, rebindable in Settings; these are the pane's own keys.
gpui::actions!(
    terminal_pane,
    [
        Copy,
        Paste,
        SelectAll,
        ScrollToTop,
        ScrollToBottom,
        ScrollPageUp,
        ScrollPageDown,
        CopyLastOutput,
        SelectLastOutput,
        FindNext,
        FindPrev,
        CloseFind,
    ]
);

/// What the programs under shells (by pid) listen on, lowest port first, for a pane's menu: the
/// browser crate reads the kernel's sockets and sets it (`set_ports_reader`).
pub type PortsReader = std::rc::Rc<dyn Fn(&[u32]) -> Vec<u16>>;

pub(crate) struct Ports(pub PortsReader);

impl gpui::Global for Ports {}

/// A pane's right-click menu opens what its dev server serves, read through `read`.
pub fn set_ports_reader(cx: &mut App, read: PortsReader) {
    cx.set_global(Ports(read));
}

/// The pane's own keys, and the terminal panel in every workspace's bottom slot.
pub fn init(cx: &mut App) {
    let pane = Some(PANE_CONTEXT);
    let mut keys = vec![
        // The shell's: not focus moves.
        KeyBinding::new("tab", NoAction, pane),
        KeyBinding::new("shift-tab", NoAction, pane),
        KeyBinding::new("escape", CloseFind, Some("TerminalFind > Input")),
    ];
    if cfg!(target_os = "macos") {
        keys.extend([
            KeyBinding::new("cmd-c", Copy, pane),
            KeyBinding::new("cmd-v", Paste, pane),
            KeyBinding::new("cmd-a", SelectAll, pane),
            KeyBinding::new("cmd-home", ScrollToTop, pane),
            KeyBinding::new("cmd-end", ScrollToBottom, pane),
            KeyBinding::new("cmd-pageup", ScrollPageUp, pane),
            KeyBinding::new("cmd-pagedown", ScrollPageDown, pane),
            KeyBinding::new("cmd-g", FindNext, Some(PANEL_CONTEXT)),
            KeyBinding::new("shift-cmd-g", FindPrev, Some(PANEL_CONTEXT)),
        ]);
    } else {
        // Linux terminals copy and paste with Ctrl+Shift+C/V: Ctrl+C is the shell's.
        keys.extend([
            KeyBinding::new("ctrl-c", NoAction, pane),
            KeyBinding::new("ctrl-shift-c", Copy, pane),
            KeyBinding::new("ctrl-shift-v", Paste, pane),
            KeyBinding::new("ctrl-home", ScrollToTop, pane),
            KeyBinding::new("ctrl-end", ScrollToBottom, pane),
        ]);
    }
    cx.bind_keys(keys);
    panel::import_legacy(cx);
    panel::register(cx);
}
