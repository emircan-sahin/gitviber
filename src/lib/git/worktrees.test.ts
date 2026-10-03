import assert from "node:assert/strict";
import { test } from "node:test";
import type { FileChange, WorktreeState } from "../api/types.ts";
import type { Pull, Worktree } from "../api/index.ts";
import { cleanable, defaultBranch, folderForBranch, hueColor, HUE_NAMES, HUES, isHueChoice, mainBackOffer, shortPath, stageable, worktreeHue, worktreeHues, worktreeName } from "./worktrees.ts";

test("short worktree paths", () => {
  const main = "/Users/me/code/app";
  assert.equal(shortPath(main, main), ".");
  assert.equal(shortPath(`${main}/.claude/worktrees/agent-1`, main), ".claude/worktrees/agent-1");
  assert.equal(shortPath("/Users/me/code/app-hotfix", main), "../app-hotfix");
  assert.equal(shortPath("/Volumes/x/app", main), "/Volumes/x/app");
  // A sibling whose name starts like the main one is not inside it.
  assert.equal(shortPath("/Users/me/code/app2", main), "../app2");
});

test("stage all leaves nested repositories out", () => {
  const file = (path: string, nested = false): FileChange => ({
    path,
    oldPath: null,
    status: "?",
    additions: null,
    deletions: null,
    oid: null,
    indexOid: null,
    conflict: null,
    mode: null,
    submodule: null,
    nested: nested ? { path: `/r/${path}` } : null,
  });
  assert.deepEqual(stageable([file("a.txt"), file("vendor/lib/", true), file("b.txt")]), { paths: ["a.txt", "b.txt"], skipped: 1 });
  assert.deepEqual(stageable([]), { paths: [], skipped: 0 });
});

test("clean up offers merged, clean, idle worktrees only", () => {
  const wt = (path: string, branch: string, more: Partial<Worktree> = {}): Worktree => ({
    path,
    head: "abc1234",
    branch,
    detached: false,
    bare: false,
    locked: false,
    lockReason: null,
    inUse: false,
    prunable: false,
    current: false,
    main: false,
    ...more,
  });
  const st = (more: Partial<WorktreeState> = {}): WorktreeState => ({ uncommitted: 0, commits: 0, merged: true, updated: null, ...more });
  const list = [
    wt("/p/app", "main", { main: true, current: true }),
    wt("/p/app.worktrees/done", "done"),
    wt("/p/app.worktrees/squashed", "squashed"),
    wt("/p/app.worktrees/dirty", "dirty"),
    wt("/p/app.worktrees/busy", "busy"),
    wt("/p/app.worktrees/locked", "locked", { locked: true }),
    wt("/p/app.worktrees/open", "open"),
    wt("/p/app.worktrees/holder", "holder"),
    wt("/p/app.worktrees/holder/.claude/worktrees/inner", "inner", { locked: true }),
    wt("/p/app.worktrees/unread", "unread"),
  ];
  const states: Record<string, WorktreeState> = {
    "/p/app": st({ merged: false }),
    "/p/app.worktrees/done": st(),
    "/p/app.worktrees/squashed": st({ merged: false, commits: 2 }),
    "/p/app.worktrees/dirty": st({ uncommitted: 3 }),
    "/p/app.worktrees/busy": st(),
    "/p/app.worktrees/locked": st(),
    "/p/app.worktrees/open": st({ merged: false, commits: 1 }),
    "/p/app.worktrees/holder": st(),
    "/p/app.worktrees/holder/.claude/worktrees/inner": st(),
  };
  const pull = { number: 12, state: "merged", headSha: "f".repeat(40) } as Pull;
  const found = cleanable(list, states, (b) => (b === "squashed" ? pull : undefined), (p) => (p.endsWith("/busy") ? 1 : 0));
  assert.deepEqual(
    found.map((c) => [c.worktree.branch, c.why, c.mergedHead]),
    [
      ["done", "merged", null],
      ["squashed", "#12 merged", "f".repeat(40)],
    ],
  );
});

test("a worktree's color comes from its folder's name until one is picked", () => {
  const w = (path: string, main = false) => ({ path, main });
  const a = worktreeHue(w("/p/app.worktrees/fix-login"), {});
  assert.ok(a && a in HUES);
  // The same name anywhere is the same color: it's the name people know it by.
  assert.equal(worktreeHue(w("/q/other.worktrees/fix-login"), {}), a);
  const names = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"].map((n) => worktreeHue(w(`/p/x/${n}`), {}));
  assert.ok(new Set(names).size >= 4, "names spread over the colors");
  assert.equal(worktreeHue(w("/p/app", true), {}), null);
  assert.equal(worktreeHue(w("/p/app", true), { "/p/app": "teal" }), "teal");
  assert.equal(worktreeHue(w("/p/app.worktrees/fix-login"), { "/p/app.worktrees/fix-login": "none" }), null);
});

test("a stored color that isn't one of the hues is dropped, whatever the storage holds", () => {
  // Read back from localStorage: hand-edited, an older build's, or another key's leftovers.
  for (const v of ["toString", "constructor", "__proto__", "hasOwnProperty", "valueOf", "Red", "", null, 3, {}]) assert.equal(isHueChoice(v), false, String(v));
  for (const v of [...Object.keys(HUES), "none"]) assert.equal(isHueChoice(v), true, v);
  for (const h of Object.keys(HUES) as (keyof typeof HUES)[]) assert.match(hueColor(h, 0.16), /^oklch\(var\(--tint-l\) var\(--tint-c\) \d+ \/ 0\.16\)$/);
});

test("clean up tells a worktree inside another from one whose name only starts the same", () => {
  const folderOf = (p: string) => p.slice(p.lastIndexOf("/") + 1);
  const wt = (path: string, more: Partial<Worktree> = {}): Worktree => ({ path, head: "abc1234", branch: folderOf(path), detached: false, bare: false, locked: false, lockReason: null, inUse: false, prunable: false, current: false, main: false, ...more });
  const st: WorktreeState = { uncommitted: 0, commits: 0, merged: true, updated: null };
  const list = [wt("/p/app", { main: true, current: true }), wt("/p/app.worktrees/fix"), wt("/p/app.worktrees/fix-2"), wt("/p/app.worktrees/fix 3"), wt("/p/app.worktrees/fix.old")];
  const states = Object.fromEntries(list.map((w) => [w.path, st]));
  const found = cleanable(list, states, () => undefined, () => 0).map((c) => c.worktree.branch);
  assert.deepEqual(found, ["fix", "fix-2", "fix 3", "fix.old"]);
});

test("same-named worktrees in two repos share a color until one is picked for either", () => {
  const a = { path: "/one/app.worktrees/fix-login", main: false };
  const b = { path: "/two/web.worktrees/fix-login", main: false };
  const picked = { [a.path]: "violet" as const };
  assert.equal(worktreeHue(a, picked), "violet");
  assert.equal(worktreeHue(b, picked), worktreeHue(b, {}));
});

test("one repo's worktrees get apart colors where their names hash alike, a pick still standing", () => {
  const w = (name: string) => ({ path: `/p/app.worktrees/${name}`, main: false });
  // Ten names, more than eight hues: the first eight are all different, then they repeat.
  const list = [{ path: "/p/app", main: true }, ...["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"].map(w)];
  const hues = worktreeHues(list, {});
  assert.equal(hues.get("/p/app"), null);
  const linked = list.slice(1).map((x) => hues.get(x.path)!);
  assert.equal(new Set(linked.slice(0, 8)).size, 8);
  assert.equal(new Set(linked).size, 8);
  // The first keeps its name's own hue; the same list always gives the same colors.
  assert.equal(linked[0], worktreeHue(list[1], {}));
  assert.deepEqual(worktreeHues(list, {}), hues);
  // A pick stands, and the others step around it.
  const picked = { [list[2].path]: linked[0], [list[3].path]: "none" as const };
  const again = worktreeHues(list, picked);
  assert.equal(again.get(list[2].path), linked[0]);
  assert.equal(again.get(list[3].path), null);
  assert.notEqual(again.get(list[1].path), linked[0]);
  assert.ok(HUE_NAMES.includes(again.get(list[1].path)!));
});

const wt = (over: Partial<Worktree>): Worktree => ({
  path: "/p/app",
  head: "abc1234",
  branch: "main",
  detached: false,
  bare: false,
  locked: false,
  lockReason: null,
  inUse: false,
  prunable: false,
  current: false,
  main: false,
  ...over,
});
const br = (name: string, over: Record<string, unknown> = {}) => ({ name, remote: name.includes("/"), remoteDefault: false, ...over });

test("a worktree is named by its branch, then its folder", () => {
  assert.deepEqual(worktreeName(wt({ path: "/p/app.worktrees/vault-tiers" })), { branch: "main", folder: "vault-tiers" });
  // A folder that only repeats the branch adds nothing; a slash in the branch is a dash in the folder.
  assert.deepEqual(worktreeName(wt({ path: "/p/app.worktrees/fix-login", branch: "fix-login" })), { branch: "fix-login", folder: null });
  assert.deepEqual(worktreeName(wt({ path: "/p/app.worktrees/feat-x", branch: "feat/x" })), { branch: "feat/x", folder: null });
  assert.equal(folderForBranch("feat/x/y"), "feat-x-y");
  assert.deepEqual(worktreeName(wt({ path: "/p/det", branch: null })), { branch: "detached @ abc1234", folder: "det" });
  assert.deepEqual(worktreeName(wt({ path: "/p/det", branch: null, head: null })), { branch: "detached @ ?", folder: "det" });
  assert.equal(worktreeName(wt({ path: "/p/x.git", branch: null, bare: true })).branch, "bare");
});

test("the default branch is origin/HEAD's, else a local main or master", () => {
  assert.equal(defaultBranch([br("trunk"), br("origin/trunk", { remoteDefault: true }), br("main")]), "trunk");
  // origin's wins over another remote's; the name keeps any slash after the remote.
  assert.equal(defaultBranch([br("up/dev", { remoteDefault: true }), br("origin/rel/1", { remoteDefault: true })]), "rel/1");
  assert.equal(defaultBranch([br("feat"), br("master")]), "master");
  assert.equal(defaultBranch([br("main"), br("master")]), "main");
  assert.equal(defaultBranch([br("feat"), br("origin/main")]), null);
});

test("Move main back is offered only when a linked worktree holds the default branch from the main folder", () => {
  const branches = [br("main"), br("feat/x"), br("origin/main", { remoteDefault: true })];
  const main = wt({ path: "/p/app", main: true, branch: "feat/x" });
  const holder = wt({ path: "/p/app-vault-tiers" });
  assert.deepEqual(mainBackOffer([main, holder], branches), { main, holder, branch: "main" });
  // The main folder already has it, or has no branch to leave, or can't be switched.
  assert.equal(mainBackOffer([{ ...main, branch: "main" }, wt({ path: "/p/other", branch: "x" })], branches), null);
  assert.equal(mainBackOffer([{ ...main, branch: null }, holder], branches), null);
  assert.equal(mainBackOffer([{ ...main, bare: true }, holder], branches), null);
  // A holder whose folder is gone is for Prune, not a move.
  assert.equal(mainBackOffer([main, { ...holder, prunable: true }], branches), null);
  assert.equal(mainBackOffer([main, holder], [br("feat/x")]), null);
});
