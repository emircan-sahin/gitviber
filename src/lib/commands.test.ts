import assert from "node:assert/strict";
import { test } from "node:test";
import { type Command, COMMANDS, canonical, cleanOverrides, commandFor, eventChord, type KeyLike, runsWhileTyping } from "./commands.ts";

const press = (key: string, code: string, mods: Partial<Omit<KeyLike, "key" | "code">> = {}): string | null =>
  eventChord({ key, code, altKey: false, shiftKey: false, metaKey: false, ctrlKey: false, ...mods });

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
  const run = (chord: string, overrides = {}) => commandFor(chord, overrides)?.id;
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

test("what still runs while typing in a text field or the terminal", () => {
  const typing = (chord: string, id: string) => runsWhileTyping(chord, COMMANDS.find((c) => c.id === id) as Command);
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
