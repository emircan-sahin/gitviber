//! The diff's commands and their default keys, as the old app's Diff and Review commands had
//! them. Bound in the diff's own key context, so the app's keymap can rebind them by name.

use crate::list::Step;
use gitviber_ui::keymap::INPUT_CONTEXT;
use gpui_kit::{Action, App, Context, Global, InteractiveElement, KeyBinding};
use std::rc::Rc;

/// The key context of a focused diff.
pub const CONTEXT: &str = "DiffView";
/// A diff whose new side is being typed into: none of its keys may take what's typed.
pub const EDITING_CONTEXT: &str = "DiffViewEditing";
/// The key context of a list of files shown as one scroll.
pub const STACKED_CONTEXT: &str = "StackedDiff";
/// The key context of the find box over a diff's lines.
pub const FIND_CONTEXT: &str = "DiffFind";

gpui_kit::actions!(
    diff_view,
    [
        NextChange,
        PrevChange,
        ToggleSplit,
        ToggleCollapse,
        ToggleWhitespace,
        StageLines,
        UnstageLines,
        DiscardLines,
        StashLines,
        RevertLines,
        CopyLines,
        CopyPatch,
        AddNote,
        ToggleViewed,
        ClearSelection,
        SelectUp,
        SelectDown,
        NextFile,
        PrevFile,
        Save,
        FindMatchCase,
        FindWholeWord,
        FindRegex,
        ScrollUp,
        ScrollDown,
        PageUp,
        PageDown,
        ScrollToTop,
        ScrollToBottom,
        ScrollLeft,
        ScrollRight,
    ]
);

/// What a diff can do for a command of the app's, which names its own actions for them
/// (`answer`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    NextChange,
    PrevChange,
    Stage,
    Unstage,
    Discard,
    AddNote,
    ToggleViewed,
    NextFile,
    PrevFile,
    Save,
}

#[derive(Default)]
pub(crate) struct Answers(pub(crate) Rc<Vec<(Box<dyn Action>, Command)>>);

impl Global for Answers {}

/// The app's actions a diff answers itself, whichever view shows it: a key bound to one reaches
/// the focused diff without its host passing it on. What a diff can't do (J/K in a single file's
/// diff) goes on to the host.
pub fn answer(cx: &mut App, commands: Vec<(Box<dyn Action>, Command)>) {
    cx.set_global(Answers(Rc::new(commands)));
}

pub(crate) fn answers(cx: &App) -> Rc<Vec<(Box<dyn Action>, Command)>> {
    cx.try_global::<Answers>()
        .map(|a| a.0.clone())
        .unwrap_or_default()
}

/// Only the keys the app's own commands don't cover: copy, picking lines with ⇧↑/⇧↓, Esc,
/// scrolling, the find box's toggles. The app hands its diff and review commands to the views
/// itself, so a rebinding applies.
pub fn init_local(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("cmd-c", CopyLines, c),
        KeyBinding::new("escape", ClearSelection, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
    ]);
    bind_scroll(cx);
    bind_find(cx);
}

/// The keys that scroll the lines, the old diff's set. Never while typing in a field over or
/// between them (find, a note); the editor has its own context.
fn bind_scroll(cx: &mut App) {
    let (top, bottom) = if cfg!(target_os = "macos") {
        ("cmd-up", "cmd-down")
    } else {
        ("ctrl-home", "ctrl-end")
    };
    for context in [CONTEXT, STACKED_CONTEXT] {
        let context = format!("{context} && !{INPUT_CONTEXT}");
        let c = Some(context.as_str());
        cx.bind_keys([
            KeyBinding::new("up", ScrollUp, c),
            KeyBinding::new("down", ScrollDown, c),
            KeyBinding::new("pageup", PageUp, c),
            KeyBinding::new("pagedown", PageDown, c),
            KeyBinding::new("shift-space", PageUp, c),
            KeyBinding::new("space", PageDown, c),
            KeyBinding::new("home", ScrollToTop, c),
            KeyBinding::new("end", ScrollToBottom, c),
            KeyBinding::new(top, ScrollToTop, c),
            KeyBinding::new(bottom, ScrollToBottom, c),
            KeyBinding::new("left", ScrollLeft, c),
            KeyBinding::new("right", ScrollRight, c),
        ]);
    }
}

/// Hands each scroll key's step to `f`.
pub(crate) fn on_scroll_keys<T: 'static, E: InteractiveElement>(
    el: E,
    f: fn(&mut T, Step, &mut Context<T>),
    cx: &Context<T>,
) -> E {
    el.on_action(cx.listener(move |t, _: &ScrollUp, _, cx| f(t, Step::Lines(-1.), cx)))
        .on_action(cx.listener(move |t, _: &ScrollDown, _, cx| f(t, Step::Lines(1.), cx)))
        .on_action(cx.listener(move |t, _: &PageUp, _, cx| f(t, Step::Pages(-1.), cx)))
        .on_action(cx.listener(move |t, _: &PageDown, _, cx| f(t, Step::Pages(1.), cx)))
        .on_action(cx.listener(move |t, _: &ScrollToTop, _, cx| f(t, Step::Start, cx)))
        .on_action(cx.listener(move |t, _: &ScrollToBottom, _, cx| f(t, Step::End, cx)))
        .on_action(cx.listener(move |t, _: &ScrollLeft, _, cx| f(t, Step::Side(-1.), cx)))
        .on_action(cx.listener(move |t, _: &ScrollRight, _, cx| f(t, Step::Side(1.), cx)))
}

/// Match Case, Whole Word and Regex while typing in the find box, as the code view's box has them.
fn bind_find(cx: &mut App) {
    let f = Some(FIND_CONTEXT);
    let (case, word, regex) = if cfg!(target_os = "macos") {
        ("alt-cmd-c", "alt-cmd-w", "alt-cmd-r")
    } else {
        ("alt-c", "alt-w", "alt-r")
    };
    cx.bind_keys([
        KeyBinding::new(case, FindMatchCase, f),
        KeyBinding::new(word, FindWholeWord, f),
        KeyBinding::new(regex, FindRegex, f),
    ]);
}

/// Every key, for a diff shown without the app (the demo).
pub fn init(cx: &mut App) {
    let c = Some(CONTEXT);
    let s = Some(STACKED_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("f7", NextChange, c),
        KeyBinding::new("alt-down", NextChange, c),
        KeyBinding::new("shift-f7", PrevChange, c),
        KeyBinding::new("alt-up", PrevChange, c),
        KeyBinding::new("alt-s", ToggleSplit, c),
        KeyBinding::new("alt-c", ToggleCollapse, c),
        KeyBinding::new("alt-cmd-s", StageLines, c),
        KeyBinding::new("alt-cmd-n", UnstageLines, c),
        KeyBinding::new("shift-cmd-backspace", DiscardLines, c),
        KeyBinding::new("cmd-c", CopyLines, c),
        KeyBinding::new("c", AddNote, c),
        KeyBinding::new("v", ToggleViewed, c),
        KeyBinding::new("escape", ClearSelection, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("cmd-s", Save, c),
        KeyBinding::new("cmd-s", Save, Some(EDITING_CONTEXT)),
        // The stacked view walks its own files; a single diff leaves J/K to the app.
        KeyBinding::new("j", NextFile, s),
        KeyBinding::new("k", PrevFile, s),
        KeyBinding::new("v", ToggleViewed, s),
        KeyBinding::new("c", AddNote, s),
        // The app's own commands, which its keymap binds where the app runs.
        KeyBinding::new("cmd-f", gitviber_ui::keymap::editor::Find, c),
        KeyBinding::new("f12", gitviber_ui::keymap::editor::GoToDefinition, c),
        KeyBinding::new("alt-f12", gitviber_ui::keymap::editor::PeekDefinition, c),
        KeyBinding::new("shift-f12", gitviber_ui::keymap::editor::GoToReferences, c),
    ]);
    bind_scroll(cx);
    bind_find(cx);
}
