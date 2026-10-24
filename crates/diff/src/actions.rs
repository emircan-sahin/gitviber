//! The diff's commands and their default keys, as the old app's Diff and Review commands had
//! them. Bound in the diff's own key context, so the app's keymap can rebind them by name.

use gpui_kit::{Action, App, Global, KeyBinding};
use std::rc::Rc;

/// The key context of a focused diff.
pub const CONTEXT: &str = "DiffView";
/// A diff whose new side is being typed into: none of its keys may take what's typed.
pub const EDITING_CONTEXT: &str = "DiffViewEditing";
/// The key context of a list of files shown as one scroll.
pub const STACKED_CONTEXT: &str = "StackedDiff";

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

/// Only the keys the app's own commands don't cover: copy, picking lines with ⇧↑/⇧↓, Esc.
/// The app hands its diff and review commands to the views itself, so a rebinding applies.
pub fn init_local(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("cmd-c", CopyLines, c),
        KeyBinding::new("escape", ClearSelection, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
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
    ]);
}
