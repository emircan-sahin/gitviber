import { test } from "node:test";
import assert from "node:assert/strict";
import { endText, endTitle, formatDuration, mouseLeftOn, parseMark, shortCommand } from "./commandMarks.ts";

test("OSC 133 marks, with their exit code and the extensions other shells add", () => {
  assert.deepEqual(parseMark("A"), { kind: "A" });
  // fish 4: kitty's click_events and cmdline_url.
  assert.deepEqual(parseMark("A;click_events=1"), { kind: "A" });
  assert.deepEqual(parseMark("C;cmdline_url=ls%20-l"), { kind: "C", command: "ls -l" });
  assert.deepEqual(parseMark("D;0"), { kind: "D", exit: 0 });
  assert.deepEqual(parseMark("D;130"), { kind: "D", exit: 130 });
  assert.deepEqual(parseMark("D;1;aid=12"), { kind: "D", exit: 1 });
  // No status given.
  assert.deepEqual(parseMark("D"), { kind: "D" });
  assert.deepEqual(parseMark("D;"), { kind: "D" });
  // Ghostty's continuation-line mark isn't read.
  assert.equal(parseMark("P;k=s"), null);
  assert.equal(parseMark(""), null);
});

test("a prompt turns off mouse reporting a dead program left on, but not under tmux", () => {
  assert.equal(mouseLeftOn("A", "normal", "any"), true);
  assert.equal(mouseLeftOn("A", "normal", "vt200"), true);
  assert.equal(mouseLeftOn("A", "normal", "x10"), true);
  assert.equal(mouseLeftOn("A", "normal", "none"), false);
  // The alternate buffer's prompt is a shell's under a full-screen program, which owns the mouse.
  assert.equal(mouseLeftOn("A", "alternate", "any"), false);
  assert.equal(mouseLeftOn("B", "normal", "any"), false);
  assert.equal(mouseLeftOn("C", "normal", "any"), false);
  assert.equal(mouseLeftOn("D", "normal", "drag"), false);
});

test("the command line a C mark carries, from fish's cmdline_url or our zsh's cmdline", () => {
  assert.deepEqual(parseMark("C"), { kind: "C" });
  // The rest of the mark, its own ; and = included.
  assert.deepEqual(parseMark("C;cmdline=echo a;b=c"), { kind: "C", command: "echo a;b=c" });
  // zsh made a newline a space; runs of them read as one.
  assert.deepEqual(parseMark("C;cmdline=for x in 1 2;   do echo $x; done "), { kind: "C", command: "for x in 1 2; do echo $x; done" });
  assert.deepEqual(parseMark("C;cmdline_url=git%20log%20%3B;aid=3"), { kind: "C", command: "git log ;" });
  assert.deepEqual(parseMark("C;cmdline_url=%E0%A4%A"), { kind: "C" });
  assert.deepEqual(parseMark("C;cmdline=   "), { kind: "C" });
  const long = parseMark(`C;cmdline=${"x".repeat(200)}`)!.command!;
  assert.equal(long.length, 80);
  assert.ok(long.endsWith("…"));
});

test("how long a command took, as its mark and a notification say it", () => {
  assert.equal(formatDuration(0), "0ms");
  assert.equal(formatDuration(349.6), "350ms");
  assert.equal(formatDuration(999.6), "1s");
  assert.equal(formatDuration(4230), "4.2s");
  assert.equal(formatDuration(5000), "5s");
  assert.equal(formatDuration(9960), "10s");
  assert.equal(formatDuration(45_400), "45s");
  assert.equal(formatDuration(60_000), "1m");
  assert.equal(formatDuration(123_000), "2m 3s");
  assert.equal(formatDuration(300_000), "5m");
  assert.equal(formatDuration(3_900_000), "1h 5m");
  assert.equal(formatDuration(7_200_000), "2h");

  assert.equal(endTitle({ ms: 2100, exit: 0 }), "Took 2.1s");
  assert.equal(endTitle({ ms: 123_000, exit: 1 }), "Failed after 2m 3s (exit code 1)");
  assert.equal(endTitle({ ms: 61_000 }), "Ended after 1m 1s");
  assert.equal(endText({ command: "pnpm test --watch", ms: 123_000, exit: 1 }), "pnpm test failed after 2m 3s (exit code 1)");
  assert.equal(endText({ command: "cargo build", ms: 45_000, exit: 0 }), "cargo build finished after 45s");
  // bash doesn't say what ran.
  assert.equal(endText({ ms: 45_000, exit: 0 }), "A command finished after 45s");
  assert.equal(endText({ ms: 45_000 }), "A command ended after 45s");
});

test("a notification names the program and subcommand, never the arguments", () => {
  assert.equal(shortCommand("pnpm test"), "pnpm test");
  assert.equal(shortCommand("cargo build --release"), "cargo build");
  assert.equal(shortCommand("git push origin main"), "git push");
  assert.equal(shortCommand("vault login s.abc"), "vault login");
  assert.equal(shortCommand('curl -H "Authorization: Bearer abc" https://x.test'), "curl");
  assert.equal(shortCommand("curl https://x.test/a?token=1"), "curl");
  assert.equal(shortCommand("deploy prod user@host"), "deploy prod");
  assert.equal(shortCommand("mysql --password=hunter2"), "mysql");
  assert.equal(shortCommand("TOKEN=abc123 pnpm test"), "pnpm test");
  assert.equal(shortCommand("./scripts/build.sh fast"), "build.sh fast");
  assert.equal(shortCommand("make a_very_long_target_name_over_the_limit"), "make");
  assert.equal(shortCommand("TOKEN=abc"), undefined);
  // Quoted words: a split mid-quote must not show its tail.
  assert.equal(shortCommand("TOKEN='tok en' deploy"), undefined);
  assert.equal(shortCommand('FOO="a b" cmd'), undefined);
  assert.equal(shortCommand('echo "sekret words"'), "echo");
  assert.equal(shortCommand("echo 'sekret words'"), "echo");
  assert.equal(shortCommand('"curl" -x'), undefined);
  assert.equal(shortCommand("echo a\\ b"), "echo");
  assert.equal(shortCommand(""), undefined);
  assert.equal(shortCommand(undefined), undefined);
  assert.equal(endText({ command: "curl -H 'X-Key: s3cret' x", ms: 45_000, exit: 0 }), "curl finished after 45s");
});
