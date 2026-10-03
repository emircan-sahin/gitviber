import { test } from "node:test";
import assert from "node:assert/strict";
import { type CommandEnd, CommandMarks } from "./commandMarks.ts";

// CommandMarks against a stand-in for xterm: the marks a shell sends in the orders real sessions
// send them, with performance.now set by hand. xterm hands the handler a whole OSC however the
// bytes were split across writes, so a split mark is the parser's, not this.

let now = 0;
Object.defineProperty(performance, "now", { value: () => now, configurable: true });

function session() {
  let osc: (data: string) => boolean = () => false;
  let line = 0;
  const term = {
    parser: { registerOscHandler: (_: number, f: typeof osc) => ((osc = f), { dispose() {} }) },
    buffer: { active: { type: "normal" as "normal" | "alternate", cursorX: 0 } },
    modes: { mouseTrackingMode: "none" },
    write() {},
    registerMarker: () => ({ line: line++, isDisposed: false, dispose() {} }),
    registerDecoration: () => undefined,
  };
  const ends: CommandEnd[] = [];
  let prompts = 0;
  new CommandMarks(
    term as never,
    () => prompts++,
    (e) => ends.push(e),
  );
  const send = (at: number, ...marks: string[]) => {
    now = at;
    for (const m of marks) osc(m);
  };
  return { send, ends, term, prompts: () => prompts };
}

test("a command's time runs from its C to its D, whatever the wall clock does", () => {
  const s = session();
  s.send(0, "A");
  s.send(1000, "C;cmdline=pnpm test");
  // The Mac's clock set back an hour (NTP, a time zone) while it ran.
  const realNow = Date.now;
  Date.now = () => realNow() - 3_600_000;
  s.send(13_500, "D;1");
  Date.now = realNow;
  assert.deepEqual(s.ends, [{ command: "pnpm test", ms: 12_500, exit: 1 }]);
});

test("no D: a killed shell's command ends at the next prompt, how unknown; a dead one never", () => {
  const s = session();
  s.send(0, "A", "C;cmdline=sleep 100");
  s.send(20_000, "A");
  assert.deepEqual(s.ends, [{ command: "sleep 100", ms: 20_000, exit: undefined }]);
  s.send(21_000, "C;cmdline=tail -f log");
  // The shell is killed: nothing more comes.
  assert.equal(s.ends.length, 1);
});

test("an empty line, ^C at the prompt and bash's D with no C end nothing", () => {
  const s = session();
  s.send(0, "D;0", "A", "D;0", "A", "D;130", "A", "B", "A");
  assert.deepEqual(s.ends, []);
  assert.equal(s.prompts(), 4);
});

test("ssh to a host that marks its prompts too, then exit", () => {
  const s = session();
  s.send(0, "A", "C;cmdline=ssh box");
  // The remote's first prompt ends the local command as far as the marks can tell.
  s.send(4000, "A");
  s.send(5000, "C;cmdline=make");
  s.send(65_000, "D;0", "A");
  s.send(66_000, "C;cmdline=exit");
  // The local shell's D for ssh lands on the remote's `exit`.
  s.send(66_200, "D;255", "A");
  assert.deepEqual(
    s.ends.map((e) => [e.command, e.ms, e.exit]),
    [
      ["ssh box", 4000, undefined],
      ["make", 60_000, 0],
      ["exit", 200, 255],
    ],
  );
});

test("a sub-shell without marks: the command that started it runs until it exits", () => {
  const s = session();
  s.send(0, "A", "C;cmdline=zsh");
  s.send(600_000, "D;0", "A");
  assert.deepEqual(s.ends, [{ command: "zsh", ms: 600_000, exit: 0 }]);
});

test("marks under a full-screen program are its shell's, not this one's", () => {
  const s = session();
  s.send(0, "A", "C;cmdline=tmux");
  s.term.buffer.active.type = "alternate";
  s.send(1000, "A", "C;cmdline=make", "D;2", "A");
  s.term.buffer.active.type = "normal";
  s.send(90_000, "D;0", "A");
  assert.deepEqual(s.ends, [{ command: "tmux", ms: 90_000, exit: 0 }]);
});

test("a second C, or a D twice, changes nothing", () => {
  const s = session();
  s.send(0, "A", "C;cmdline=first");
  s.send(5000, "C;cmdline=second");
  s.send(9000, "D;0", "D;1");
  assert.deepEqual(s.ends, [{ command: "first", ms: 9000, exit: 0 }]);
});

test("a command line of any size or content, from either shell", () => {
  const s = session();
  s.send(0, "A", `C;cmdline_url=${"%41".repeat(100_000)}`);
  s.send(1, "D;0", "A", `C;cmdline=${"ş".repeat(5000)}`);
  s.send(2, "D;0", "A", "C;cmdline_url=%ZZ");
  s.send(3, "D;0", "A", "C;cmdline=a;cmdline_url=b%20c");
  s.send(4, "D;0");
  assert.deepEqual(
    s.ends.map((e) => e.command),
    [`${"A".repeat(79)}…`, `${"ş".repeat(79)}…`, undefined, "a;cmdline_url=b%20c"],
  );
});

test("a thousand commands: each ends once", () => {
  const s = session();
  for (let i = 0; i < 1000; i++) {
    s.send(i * 10, "A", `C;cmdline=echo ${i}`);
    s.send(i * 10 + 5, `D;${i % 3}`);
  }
  assert.equal(s.ends.length, 1000);
  assert.deepEqual(s.ends[999], { command: "echo 999", ms: 5, exit: 0 });
});
