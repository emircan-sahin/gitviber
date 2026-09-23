import assert from "node:assert/strict";
import { test } from "node:test";
import { canonical, cleanOverrides, commandFor, eventChord, formatChordFor, type KeyLike, menuAccelerator } from "./commands.ts";

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
  assert.equal(commandFor("cmd+1", { "view.toggleGitPanel": ["cmd+1"] })?.id, "view.changes");
  assert.equal(commandFor("cmd+enter", {}), undefined, "local commands never run globally");
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
