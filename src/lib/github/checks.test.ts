import assert from "node:assert/strict";
import { test } from "node:test";
import type { CiCheck } from "../api/github.ts";
import { checkNote, checksSummary, checksVerdict, sortChecks } from "./checks.ts";

const check = (name: string, state: string, more: Partial<CiCheck> = {}): CiCheck => ({
  name,
  state,
  status: state === "pending" ? "in_progress" : "",
  description: "",
  app: "",
  startedAt: null,
  completedAt: null,
  url: null,
  id: null,
  ...more,
});

test("the summary counts passes out of all, then what didn't pass", () => {
  const checks = [check("a", "success"), check("b", "success"), check("c", "success"), check("d", "failure"), check("e", "pending")];
  assert.equal(checksSummary(checks), "3/5 passed · 1 failing · 1 running");
  assert.equal(checksSummary([check("a", "success"), check("b", "success")]), "2/2 passed");
  assert.equal(checksSummary([check("a", "success"), check("b", "skipped"), check("c", "neutral"), check("d", "timed_out")]), "1/4 passed · 1 failing · 1 skipped · 1 neutral");
});

test("the verdict: any failure fails, else anything running runs", () => {
  assert.equal(checksVerdict([]), undefined);
  assert.equal(checksVerdict([check("a", "pending"), check("b", "cancelled")]), "failure");
  assert.equal(checksVerdict([check("a", "pending"), check("b", "success")]), "pending");
  assert.equal(checksVerdict([check("a", "skipped"), check("b", "success")]), "success");
});

test("failing checks lead, then running ones, each in the order GitHub gave", () => {
  const sorted = sortChecks([check("ok", "success"), check("run1", "pending"), check("bad1", "failure"), check("run2", "pending"), check("bad2", "action_required")]);
  assert.deepEqual(
    sorted.map((c) => c.name),
    ["bad1", "bad2", "run1", "run2", "ok"],
  );
});

test("a running check says what it waits on and for how long", () => {
  const now = Date.parse("2024-05-01T12:10:00Z");
  assert.equal(checkNote(check("deploy", "pending", { status: "waiting", startedAt: "2024-05-01T11:05:00Z" }), now), "Waiting for approval · 1h 5m");
  assert.equal(checkNote(check("build", "pending", { status: "queued", startedAt: "2024-05-01T12:09:30Z" }), now), "Queued · <1m");
  assert.equal(checkNote(check("preview", "pending", { status: "pending", description: "Building" }), now), "Pending · Building");
  assert.equal(checkNote(check("odd", "pending", { status: "something_new" }), now), "Pending");
});

test("a done check says how it ended and how long it took, when it knows", () => {
  const failed = check("lint", "failure", { description: "1 error", startedAt: "2024-05-01T12:00:00Z", completedAt: "2024-05-01T12:03:10Z" });
  assert.equal(checkNote(failed), "1 error · 3m");
  assert.equal(checkNote(check("ci/legacy", "failure", { description: "Build crashed", startedAt: "2024-05-01T12:00:00Z" })), "Build crashed");
  assert.equal(checkNote(check("ok", "success")), "");
});
