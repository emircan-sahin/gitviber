import assert from "node:assert/strict";
import { test } from "node:test";
import {
  bindingsFor,
  type Command,
  COMMANDS,
  canonical,
  cleanOverrides,
  commandFor,
  eventChord,
  eventChords,
  formatChordFor,
  isReserved,
  type KeyLike,
  menuAccelerator,
  runsWhileTyping,
  runsInTerminal,
  takenFromTerminal,
} from "./commands.ts";

const press = (key: string, code: string, mods: Partial<Omit<KeyLike, "key" | "code">> = {}, mac = true): string | null =>
  eventChord({ key, code, altKey: false, shiftKey: false, metaKey: false, ctrlKey: false, ...mods }, mac);

test("US layout", () => {
  assert.equal(press("b", "KeyB", { metaKey: true }), "cmd+b");
  assert.equal(press("B", "KeyB", { metaKey: true }), "cmd+b", "caps lock");
  // ⌥ turns letters into symbols; the physical key is used.
  assert.equal(press("∫", "KeyB", { altKey: true, metaKey: true }), "alt+cmd+b");
  assert.equal(press("E", "KeyE", { shiftKey: true, metaKey: true }), "shift+cmd+e");
  assert.equal(press("=", "Equal", { metaKey: true }), "cmd+=");
  assert.equal(press("+", "Equal", { shiftKey: true, metaKey: true }), "shift+cmd+=");
  assert.equal(press("-", "Minus", { metaKey: true }), "cmd+-");
  assert.equal(press("≠", "Equal", { altKey: true, metaKey: true }), "alt+cmd+=");
  assert.equal(press("–", "Minus", { altKey: true, metaKey: true }), "alt+cmd+-");
  assert.equal(press(",", "Comma", { metaKey: true }), "cmd+,");
  assert.equal(press("ArrowDown", "ArrowDown", { altKey: true }), "alt+down");
  assert.equal(press("F7", "F7", { shiftKey: true }), "shift+f7");
  assert.equal(press("Enter", "Enter", { metaKey: true }), "cmd+enter");
  assert.equal(press("j", "KeyJ"), "j");
  assert.equal(press("Meta", "MetaLeft", { metaKey: true }), null);
  // ⇧ turns the bracket keys into braces; the chord keeps the key's own name.
  assert.equal(press("}", "BracketRight", { shiftKey: true, metaKey: true }), "shift+cmd+]");
  assert.equal(press("{", "BracketLeft", { shiftKey: true, metaKey: true }), "shift+cmd+[");
  assert.equal(press("Tab", "Tab", { ctrlKey: true, shiftKey: true }), "ctrl+shift+tab");
  assert.equal(press("1", "Digit1", { ctrlKey: true }), "ctrl+1");
  assert.equal(press("ArrowRight", "ArrowRight", { metaKey: true }), "cmd+right");
});

test("Turkish Q layout", () => {
  // Turkish "-" and "*" sit where US has "=" and "-".
  assert.equal(press("-", "Equal", { metaKey: true }), "cmd+-");
  assert.equal(press("-", "Equal"), "-");
  // ⌥⌘- must shrink the code font, not grow it (the physical key is US "=").
  assert.equal(press("–", "Equal", { altKey: true, metaKey: true }), "alt+cmd+-");
  assert.equal(press("ş", "Semicolon", { metaKey: true }), "cmd+ş");
  assert.equal(press(",", "Backslash", { metaKey: true }), "cmd+,");
});

test("German layout", () => {
  // Z and Y are swapped; ⌥Z must stay ⌥Z once the layout has been seen.
  assert.equal(press("z", "KeyY"), "z");
  assert.equal(press("Ω", "KeyY", { altKey: true }), "alt+z");
  // ⌥ output that is plain ASCII is kept as typed.
  assert.equal(press("@", "KeyL", { altKey: true }), "alt+@");
  // "+" has its own key; "=" is ⇧0.
  assert.equal(press("+", "BracketRight", { metaKey: true }), "cmd+=");
  assert.equal(press("=", "Digit0", { shiftKey: true, metaKey: true }), "shift+cmd+=");
});

test("canonical orders modifiers and rejects unknown ones", () => {
  assert.equal(canonical("cmd+shift+E"), "shift+cmd+e");
  assert.equal(canonical("cmd+alt+ctrl+-"), "ctrl+alt+cmd+-");
  assert.equal(canonical("j"), "j");
  assert.equal(canonical("Meta+b"), null);
  assert.equal(canonical("cmd+cmd+b"), null);
  assert.equal(canonical("cmd+"), null);
});

test("cleanOverrides drops unknown commands and bad chords", () => {
  assert.deepEqual(
    cleanOverrides({ "view.toggleGitPanel": ["cmd+shift+g"], "tab.close": [], "gone.command": ["cmd+g"], "view.changes": ["Meta+b"], "view.history": "cmd+2" }),
    { "view.toggleGitPanel": ["shift+cmd+g"], "tab.close": [] },
  );
  assert.deepEqual(cleanOverrides(null), {});
  assert.deepEqual(cleanOverrides(["cmd+b"]), {});
});

test("the first listed command wins a shared chord", () => {
  assert.equal(commandFor("cmd+1", { "view.toggleGitPanel": ["cmd+1"] })?.id, "view.toggleGitPanel");
  assert.equal(commandFor("cmd+b", { "tab.goto1": ["cmd+b"] })?.id, "view.toggleGitPanel");
  assert.equal(commandFor("cmd+enter", {}), undefined, "local commands never run globally");
});

test("⌘1–⌘9 pick tabs; the git panel's views moved to ⌃1–⌃4", () => {
  const run = (chord: string, overrides = {}) => commandFor(chord, overrides, true)?.id;
  assert.equal(run("cmd+1"), "tab.goto1");
  assert.equal(run("cmd+8"), "tab.goto8");
  assert.equal(run("cmd+9"), "tab.last");
  assert.equal(run("ctrl+1"), "view.changes");
  assert.equal(run("ctrl+4"), "view.issues");
  for (const chord of ["cmd+right", "shift+cmd+]", "ctrl+tab"]) assert.equal(run(chord), "tab.next");
  for (const chord of ["cmd+left", "shift+cmd+[", "ctrl+shift+tab"]) assert.equal(run(chord), "tab.prev");
  // Only the defaults moved: someone who bound ⌘1 to Changes keeps it.
  assert.equal(run("cmd+1", { "view.changes": ["cmd+1"] }), "view.changes");
  assert.equal(run("ctrl+1", { "view.changes": ["alt+cmd+1"] }), undefined);
});

test("off macOS: views on Alt+1–4, tabs on Ctrl+Tab (the Super key is the OS's)", () => {
  const run = (chord: string, overrides = {}) => commandFor(chord, overrides, false)?.id;
  assert.equal(run("cmd+1"), "tab.goto1", "Ctrl+1");
  assert.equal(run("alt+1"), "view.changes");
  assert.equal(run("alt+4"), "view.issues");
  assert.equal(run("ctrl+1"), undefined, "Super+1");
  assert.equal(run("cmd+tab"), "tab.next");
  assert.equal(run("shift+cmd+tab"), "tab.prev");
  assert.equal(run("shift+cmd+]"), "tab.next");
  assert.equal(run("ctrl+tab"), undefined, "Super+Tab");
  // Ctrl+← / Ctrl+→ move by word, as everywhere there.
  assert.equal(run("cmd+right"), undefined);
  assert.equal(press("Tab", "Tab", { ctrlKey: true }, false), "cmd+tab");
  assert.deepEqual(bindingsFor("view.changes", {}, false), ["alt+1"]);
  assert.deepEqual(bindingsFor("view.changes", { "view.changes": ["cmd+1"] }, false), ["cmd+1"]);
});

const byId = (id: string) => COMMANDS.find((c) => c.id === id) as Command;

test("what still runs while typing in a text field or the terminal (macOS)", () => {
  const typing = (chord: string, id: string) => runsWhileTyping(chord, byId(id), true);
  // ⌘←/⌘→ move the cursor (start/end of line in the terminal); the other tab keys work everywhere.
  assert.equal(typing("cmd+right", "tab.next"), false);
  assert.equal(typing("cmd+left", "tab.prev"), false);
  assert.equal(typing("shift+cmd+]", "tab.next"), true);
  assert.equal(typing("ctrl+tab", "tab.next"), true);
  assert.equal(typing("ctrl+shift+tab", "tab.prev"), true);
  assert.equal(typing("cmd+1", "tab.goto1"), true);
  assert.equal(typing("ctrl+1", "view.changes"), true);
  // ⌃+letter edits the line (⌃A, ⌃E); plain letters and ⌥ chords type.
  assert.equal(typing("ctrl+a", "view.changes"), false);
  assert.equal(typing("j", "review.nextFile"), false);
  assert.equal(typing("alt+down", "diff.nextChange"), false);
  assert.equal(typing("f7", "diff.nextChange"), true);
  // Text fields keep ⌘Z whatever it's bound to.
  assert.equal(typing("cmd+z", "git.undo"), false);
});

test("what still runs while typing (elsewhere: cmd is the physical Ctrl)", () => {
  const typing = (chord: string, id: string) => runsWhileTyping(chord, byId(id), false);
  assert.equal(typing("cmd+tab", "tab.next"), true);
  assert.equal(typing("cmd+1", "tab.goto1"), true);
  assert.equal(typing("cmd+b", "view.toggleGitPanel"), true);
  assert.equal(typing("cmd+right", "tab.next"), false, "Ctrl+→ moves by word");
  assert.equal(typing("alt+1", "view.changes"), false, "Alt types characters on some layouts");
  assert.equal(typing("cmd+z", "git.undo"), false);
});

test("the terminal hands the app its Ctrl chords, except Ctrl+letter", () => {
  const taken = (chord: string, id: string, mac: boolean) => takenFromTerminal(chord, byId(id), mac);
  assert.equal(taken("ctrl+tab", "tab.next", true), true);
  assert.equal(taken("ctrl+1", "view.changes", true), true);
  assert.equal(taken("ctrl+a", "view.changes", true), false, "⌃A is the shell's");
  // ⌘ chords reach the app anyway on macOS; this is only about ⌃.
  assert.equal(taken("cmd+1", "tab.goto1", true), false);
  assert.equal(taken("cmd+tab", "tab.next", false), true, "Ctrl+Tab");
  assert.equal(taken("cmd+1", "tab.goto1", false), true, "Ctrl+1");
  assert.equal(taken("cmd+b", "view.toggleGitPanel", false), false, "Ctrl+B is the shell's (tmux)");
  assert.equal(taken("cmd+left", "tab.prev", false), false, "Ctrl+← moves by word");
  assert.equal(taken("alt+1", "view.changes", false), false);
  assert.equal(taken("shift+cmd+t", "terminal.new", false), true, "Ctrl+Shift+T, as in Linux terminals");
});

test("off macOS, cmd is Ctrl and ctrl is the Windows / Super key", () => {
  assert.equal(press("b", "KeyB", { ctrlKey: true }, false), "cmd+b");
  assert.equal(press("E", "KeyE", { ctrlKey: true, shiftKey: true }, false), "shift+cmd+e");
  assert.equal(press("b", "KeyB", { metaKey: true }, false), "ctrl+b");
  assert.equal(menuAccelerator("shift+cmd+e", false), "shift+ctrl+e");
  assert.equal(menuAccelerator("ctrl+enter", false), "super+enter");
  assert.equal(menuAccelerator("shift+cmd+e", true), "shift+cmd+e");
  assert.equal(formatChordFor("shift+cmd+e", false), "Ctrl+Shift+E");
  assert.equal(formatChordFor("alt+down", false), "Alt+↓");
  assert.equal(formatChordFor("shift+cmd+e", true), "⇧⌘E");
});

test("with the terminal focused, a key goes to the app or the shell, never both", () => {
  const app = (chord: string, id: string, mac: boolean) => runsInTerminal(chord, byId(id), mac);
  for (const mac of [true, false]) {
    // F-keys are the shell's (htop, mc), though a text field lets them through.
    assert.equal(app("f6", "view.focusNextPanel", mac), false);
    assert.equal(app("shift+f6", "view.focusPrevPanel", mac), false);
    assert.equal(app("f7", "diff.nextChange", mac), false);
    assert.equal(runsWhileTyping("f6", byId("view.focusNextPanel"), mac), true);
    assert.equal(app("j", "review.nextFile", mac), false);
    assert.equal(app("alt+down", "diff.nextChange", mac), false);
  }
  // What xterm skips: ⌘ chords and the ⌃ chords it hands over.
  assert.equal(app("cmd+1", "tab.goto1", true), true);
  assert.equal(app("shift+cmd+]", "tab.next", true), true);
  assert.equal(app("ctrl+tab", "tab.next", true), true);
  assert.equal(app("ctrl+a", "view.changes", true), false);
  assert.equal(app("cmd+left", "tab.prev", true), false, "start of line");
  // Elsewhere the physical Ctrl is "cmd": Ctrl+letter is the shell's, Super is the app's.
  assert.equal(app("cmd+tab", "tab.next", false), true);
  assert.equal(app("cmd+1", "tab.goto1", false), true);
  assert.equal(app("cmd+b", "view.toggleGitPanel", false), false);
  assert.equal(app("ctrl+b", "view.toggleGitPanel", false), true);
});

test("a punctuation key also counts as its US name; letters and digits only as typed", () => {
  const chords = (key: string, code: string, mods: Partial<Omit<KeyLike, "key" | "code">> = {}) =>
    eventChords({ key, code, altKey: false, shiftKey: false, metaKey: false, ctrlKey: false, ...mods }, true);
  // Turkish Q types " on the US ` key: ⌃` still toggles the terminal.
  assert.deepEqual(chords('"', "Backquote", { ctrlKey: true }), ["ctrl+\"", "ctrl+`"]);
  assert.equal(commandFor("ctrl+`", {}, true)?.id, "terminal.toggle");
  assert.deepEqual(chords("`", "Backquote", { ctrlKey: true }), ["ctrl+`"]);
  // German ⌘Y is the US Z key, and must not undo.
  assert.deepEqual(chords("y", "KeyZ", { metaKey: true }), ["cmd+y"]);
  // Without ⌘ or ⌃ a key types: US "?" is not "/", German "-" (the US / key) is not "/".
  assert.deepEqual(chords("?", "Slash", { shiftKey: true }), ["shift+?"]);
  assert.deepEqual(chords("-", "Slash"), ["-"]);
});

test("the terminal's own keys only run there, and keep ⌘W from closing a tab only there", () => {
  assert.equal(commandFor("cmd+d", {}, true), undefined);
  assert.equal(commandFor("cmd+w", {}, true)?.id, "tab.close");
  assert.deepEqual(bindingsFor("terminal.split", {}, true), ["cmd+d"]);
  // ⌃` reaches the app from inside the terminal instead of sending NUL.
  assert.ok(takenFromTerminal("ctrl+`", byId("terminal.toggle"), true));
  // Off macOS Ctrl+letter is the shell's and Super+D / Super+W the desktop's (KDE): Ctrl+Shift.
  assert.deepEqual(bindingsFor("terminal.split", {}, false), ["shift+cmd+d"]);
  assert.equal(commandFor("shift+cmd+w", {}, false), undefined);
  assert.equal(commandFor("cmd+w", {}, false)?.id, "tab.close");
});

test("only macOS reserves chords", () => {
  assert.equal(isReserved("cmd+q", true), true);
  assert.equal(isReserved("cmd+h", true), true);
  assert.equal(isReserved("cmd+b", true), false);
  // Off macOS "cmd" is Ctrl, which nothing takes from the page.
  assert.equal(isReserved("cmd+h", false), false);
  assert.equal(isReserved("cmd+m", false), false);
});

test("no two global commands share a default, and macOS takes none of them", () => {
  for (const mac of [true, false]) {
    const owner = new Map<string, string>();
    for (const c of COMMANDS) {
      if ("local" in c) continue;
      for (const chord of bindingsFor(c.id, {}, mac)) {
        assert.equal(owner.get(chord), undefined, `${chord} is ${owner.get(chord)}'s and ${c.id}'s (mac: ${mac})`);
        assert.equal(isReserved(chord, mac), false, `${chord} (${c.id})`);
        owner.set(chord, c.id);
      }
    }
  }
});

test("stage, unstage and discard the change at the cursor: VS Code's second keys, with ⌥", () => {
  for (const mac of [true, false]) {
    assert.equal(commandFor("alt+cmd+s", {}, mac)?.id, "diff.stageChange");
    assert.equal(commandFor("alt+cmd+n", {}, mac)?.id, "diff.unstageChange");
    assert.equal(commandFor("alt+cmd+r", {}, mac)?.id, "diff.discardChange");
  }
  // ⌘R stays Reload Window.
  assert.equal(commandFor("cmd+r", {}, true)?.id, "window.reload");
  // ⌥ turns S into ß; the physical key counts.
  assert.equal(press("ß", "KeyS", { altKey: true, metaKey: true }), "alt+cmd+s");
  assert.equal(formatChordFor("alt+cmd+s", true), "⌥⌘S");
  assert.equal(formatChordFor("alt+cmd+r", false), "Ctrl+Alt+R");
  assert.equal(menuAccelerator("alt+cmd+n", false), "alt+ctrl+n");
});
