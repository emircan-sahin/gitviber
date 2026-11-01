//! Every default key of both presets pressed from every place focus can be: it runs the command
//! the command table and the chord rules say it runs there, or nothing where the place keeps the
//! key (the shell's keys in the terminal, the editing keys in a text field, the page's in a
//! browser page). The expectations come from the rules, not from the keymap GPUI was given, so a
//! binding under the wrong context, a key another crate's binding takes, or a rule broken shows.

use gitviber_ui::keymap::chord::{
    is_reserved, runs_in_editor, runs_in_terminal, runs_while_typing, to_keystroke,
};
use gitviber_ui::keymap::{
    self, Command, Overrides, Preset, SYSTEM_COMMANDS, all_commands, bindings_for,
};
use gpui_kit::{KeyBindingContextPredicate, KeyContext, TestAppContext};

use super::{At, TAB_NUMBERS, press, setup_with};

/// The places a key is pressed from: the code view, its find bar, the commit box, a terminal
/// pane, a browser page and its address bar, a rendered document, the explorer, the git panel's
/// list, a diff, the code area's tab strip, the command palette.
const PLACES: [At; 12] = [
    At::Code,
    At::FindBar,
    At::Summary,
    At::Pane,
    At::Page,
    At::Address,
    At::Reading,
    At::Explorer,
    At::Changes,
    At::Diff,
    At::Tabs,
    At::Palette,
];

/// Where focus has tabs of its own to pick by number: the terminal's, else the code area's.
const TAB_AREAS: [&str; 2] = ["CodeArea", "TerminalPanel"];

/// The keys every preset gives the tabs of the place focus is in.
fn tab_number_chords() -> Vec<(String, &'static str)> {
    (1..=9)
        .map(|n| (format!("cmd+{n}"), TAB_NUMBERS[n - 1]))
        .collect()
}

/// Cases the rules don't describe, each with why.
const EXCEPTIONS: &[(&str, At, &str)] = &[
    (
        "cmd+f",
        At::Code,
        "the code editor opens its own search on it, the box editor.find opens there",
    ),
    (
        "cmd+a",
        At::Summary,
        "the commit box selects its text, not the list's changes around it",
    ),
];

fn has(stack: &[KeyContext], name: &str) -> bool {
    stack.iter().any(|c| c.contains(name))
}

fn matches(predicate: &str, stack: &[KeyContext]) -> Option<usize> {
    KeyBindingContextPredicate::parse(predicate)
        .unwrap()
        .depth_of(stack)
}

/// What a browser page leaves the app (the browser's keys.rs): ⌘ and ⌃ chords but its editing
/// ones, which are the page's whoever binds them; macOS's own go to the menu bar.
fn page_leaves(chord: &str) -> bool {
    let (mods, key) = chord.rsplit_once('+').unwrap_or(("", chord));
    let mods: Vec<&str> = mods.split('+').filter(|m| !m.is_empty()).collect();
    let (cmd, ctrl, alt, shift) = (
        mods.contains(&"cmd"),
        mods.contains(&"ctrl"),
        mods.contains(&"alt"),
        mods.contains(&"shift"),
    );
    if !(cmd || ctrl) {
        return false;
    }
    let arrow = matches!(key, "left" | "right" | "up" | "down");
    let editing = cmd
        && !ctrl
        && (!alt
            && !shift
            && matches!(
                key,
                "c" | "v" | "x" | "a" | "backspace" | "delete" | "enter"
            )
            || !alt && key == "z"
            || arrow);
    !editing
}

/// The command a global chord runs from `stack`, by the chord rules.
fn global_runs(chord: &str, c: &Command, stack: &[KeyContext]) -> bool {
    if has(stack, "Terminal") {
        return runs_in_terminal(chord, c, true);
    }
    if has(stack, "Input") {
        let editor = stack.last().and_then(|l| l.get("mode")).map(|m| m.as_ref()) == Some("editor");
        return runs_while_typing(chord, c, true) || (editor && runs_in_editor(chord, c));
    }
    true
}

/// What `chord` should run from `stack` among `bound` (the commands with it, in the table's
/// order): the deepest place's own command, else the first global one the rules let run there,
/// a tab number before the others where focus has tabs of its own. None: the place keeps it.
fn want(chord: &str, bound: &[&'static Command], stack: &[KeyContext]) -> Option<String> {
    if has(stack, "BrowserPage") && !page_leaves(chord) {
        return None;
    }
    let local = bound
        .iter()
        .filter_map(|c| Some((matches(c.local?, stack)?, *c)))
        .max_by_key(|(depth, _)| *depth);
    if let Some((_, c)) = local {
        return Some(c.id.to_string());
    }
    let globals: Vec<&Command> = bound
        .iter()
        .copied()
        .filter(|c| c.local.is_none() && global_runs(chord, c, stack))
        .collect();
    let in_area = TAB_AREAS.iter().any(|a| has(stack, a));
    let tab = globals
        .iter()
        .find(|c| in_area && TAB_NUMBERS.contains(&c.id) && is_tab_number_key(chord));
    let c = tab.or(globals.first())?;
    // The terminal panel takes its tab keys for its own tabs on ⌘1–⌘9.
    if TAB_NUMBERS.contains(&c.id) && has(stack, "TerminalPanel") && is_tab_number_key(chord) {
        return Some(format!("terminal: {}", c.id));
    }
    Some(c.id.to_string())
}

fn is_tab_number_key(chord: &str) -> bool {
    tab_number_chords().iter().any(|(k, _)| k == chord)
}

/// Every chord of `preset` on macOS with the commands that have it, in the table's order, and
/// the tab numbers every preset gives.
fn chords(preset: Preset) -> Vec<(String, Vec<&'static Command>)> {
    let mut out: Vec<(String, Vec<&'static Command>)> = Vec::new();
    let mut add =
        |chord: String, c: &'static Command| match out.iter_mut().find(|(k, _)| *k == chord) {
            Some((_, list)) if !list.iter().any(|o| o.id == c.id) => list.push(c),
            Some(_) => {}
            None => out.push((chord, vec![c])),
        };
    for c in all_commands() {
        for chord in bindings_for(c, preset, &Overrides::new(), true) {
            if !is_reserved(&chord, true) {
                add(chord, c);
            }
        }
    }
    for c in SYSTEM_COMMANDS {
        for chord in c.default_keys(true) {
            add(chord.to_string(), c);
        }
    }
    for (chord, id) in tab_number_chords() {
        add(chord, keymap::command(id).unwrap());
    }
    // In the table's order, as the keymap ranks a shared chord.
    let order = |c: &Command| {
        all_commands()
            .chain(SYSTEM_COMMANDS)
            .position(|o| o.id == c.id)
    };
    for (_, list) in &mut out {
        list.sort_by_key(|c| order(c));
    }
    out
}

/// Each chord from each place: how many cases ran, and the ones that went wrong.
fn run(preset: Preset, cx: &mut TestAppContext) -> (usize, Vec<String>) {
    let (shell, log, cx) = setup_with(preset, cx);
    let mut failed = Vec::new();
    let mut cases = 0;
    let chords = chords(preset);
    for at in PLACES {
        let stack = {
            cx.update(|window, cx| {
                let focus = shell.read(cx).focus_of(at, cx);
                window.focus(&focus, cx);
            });
            cx.run_until_parked();
            cx.update(|window, _| window.context_stack())
        };
        for (chord, bound) in &chords {
            if EXCEPTIONS.iter().any(|(k, a, _)| k == chord && *a == at) {
                continue;
            }
            cases += 1;
            let want = want(chord, bound, &stack);
            let got = press(cx, &shell, &log, at, &to_keystroke(chord, true));
            if got.first() != want.as_ref() || got.len() > 1 {
                failed.push(format!("{chord} at {at:?}: got {got:?}, want {want:?}"));
            }
        }
    }
    (cases, failed)
}

#[gpui::test]
fn every_vscode_key_reaches_its_command_where_it_should(cx: &mut TestAppContext) {
    let (cases, failed) = run(Preset::VsCode, cx);
    assert!(
        failed.is_empty(),
        "VS Code, {} of {cases}:\n{}",
        failed.len(),
        failed.join("\n")
    );
    eprintln!("VS Code: {cases} cases");
}

#[gpui::test]
fn every_classic_key_reaches_its_command_where_it_should(cx: &mut TestAppContext) {
    let (cases, failed) = run(Preset::Classic, cx);
    assert!(
        failed.is_empty(),
        "Classic, {} of {cases}:\n{}",
        failed.len(),
        failed.join("\n")
    );
    eprintln!("Classic: {cases} cases");
}
