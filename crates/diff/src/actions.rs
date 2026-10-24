//! The diff's commands and their default keys, as the old app's Diff and Review commands had
//! them. Bound in the diff's own key context, so the app's keymap can rebind them by name.

use gpui_kit::{App, KeyBinding};

/// The key context of a focused diff.
pub const CONTEXT: &str = "DiffView";
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
        // The stacked view walks its own files; a single diff leaves J/K to the app.
        KeyBinding::new("j", NextFile, s),
        KeyBinding::new("k", PrevFile, s),
        KeyBinding::new("v", ToggleViewed, s),
        KeyBinding::new("c", AddNote, s),
    ]);
}
