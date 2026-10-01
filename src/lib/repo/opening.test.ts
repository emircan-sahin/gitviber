import assert from "node:assert/strict";
import { test } from "node:test";
import { fallbackFor, latestOnly } from "./opening.ts";

const tick = () => new Promise((r) => setTimeout(r, 0));

test("opens run one at a time, and one asked again before its turn is skipped", async () => {
  const opens = latestOnly();
  const ran: string[] = [];
  let finishA!: () => void;
  const a = opens.run(() => new Promise<string>((done) => (finishA = () => done("a"))).then((r) => (ran.push(r), r)));
  await tick();
  // a has started; b is overtaken by c before its turn comes.
  const b = opens.run(async () => (ran.push("b"), "b"));
  const c = opens.run(async () => (ran.push("c"), "c"));
  assert.ok(opens.busy());
  await tick();
  assert.deepEqual(ran, [], "nothing runs alongside a");
  finishA();
  assert.equal(await a.turn, "a");
  assert.equal(await b.turn, null);
  assert.equal(await c.turn, "c");
  assert.deepEqual(ran, ["a", "c"]);
  assert.equal(a.latest(), false);
  assert.equal(c.latest(), true);
  assert.ok(!opens.busy());
});

test("a failed open doesn't stop the next", async () => {
  const opens = latestOnly();
  const a = opens.run(() => Promise.reject(new Error("not a repo")));
  await tick();
  const b = opens.run(async () => "b");
  await assert.rejects(a.turn);
  assert.equal(await b.turn, "b");
  assert.ok(!opens.busy());
});

test("a deleted worktree falls back to its project", () => {
  const projects = ["/code/app", "/code/site"];
  // Its main worktree, wherever the worktree was.
  assert.equal(fallbackFor(projects, "/code/app.worktrees/fix", "/code/app"), "/code/app");
  // Launch knows only the path: the project it was inside, else the first.
  assert.equal(fallbackFor(projects, "/code/site/.claude/worktrees/agent"), "/code/site");
  assert.equal(fallbackFor(projects, "/elsewhere/x"), "/code/app");
  assert.equal(fallbackFor(projects, null), "/code/app");
  // The main worktree itself gone: never itself again.
  assert.equal(fallbackFor(projects, "/code/app", "/code/app"), "/code/site");
  assert.equal(fallbackFor(["/code/app"], "/code/app", "/code/app"), undefined);
});
