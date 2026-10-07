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

test("a start in the future (clock skew) reads as under a minute, a multi-day run in days", () => {
  const now = Date.parse("2024-05-01T12:00:00Z");
  assert.equal(checkNote(check("build", "pending", { status: "in_progress", startedAt: "2024-05-01T12:05:00Z" }), now), "In progress · <1m");
  const long = check("soak", "success", { startedAt: "2024-05-01T00:00:00Z", completedAt: "2024-05-04T06:00:00Z" });
  assert.equal(checkNote(long), "3d");
  const backwards = check("odd", "failure", { startedAt: "2024-05-01T12:05:00Z", completedAt: "2024-05-01T12:00:00Z" });
  assert.equal(checkNote(backwards), "<1m");
});

test("a timestamp that doesn't parse leaves the duration out", () => {
  const now = Date.parse("2024-05-01T12:00:00Z");
  assert.equal(checkNote(check("build", "pending", { status: "queued", startedAt: "not a date" }), now), "Queued");
  assert.equal(checkNote(check("lint", "failure", { startedAt: "2024-05-01T12:00:00Z", completedAt: "" + "garbage" })), "");
});

test("startup_failure is a failure, as GitHub's rollup counts it", () => {
  const checks = [check("a", "success"), check("workflow", "startup_failure")];
  assert.equal(checksVerdict(checks), "failure");
  assert.equal(checksSummary(checks), "1/2 passed · 1 failing");
  assert.deepEqual(sortChecks(checks).map((c) => c.name), ["workflow", "a"]);
});

test("every check is counted somewhere in the summary", () => {
  const checks = [check("a", "success"), check("b", "")];
  const summary = checksSummary(checks);
  const counted = [...summary.matchAll(/(\d+) (?:failing|running|skipped|neutral)/g)].reduce((n, m) => n + Number(m[1]), Number(summary.split("/")[0]));
  assert.equal(counted, checks.length, summary);
});

test("stale counts as neutral, and neutral or skipped alone pass", () => {
  assert.equal(checksSummary([check("a", "stale"), check("b", "skipped")]), "0/2 passed · 1 skipped · 1 neutral");
  assert.equal(checksVerdict([check("a", "stale"), check("b", "neutral"), check("c", "skipped")]), "success");
});

test("500 checks with unicode names: sorted stably, counted exactly, quickly", () => {
  const states = ["success", "failure", "pending", "skipped", "neutral", "cancelled", "timed_out"];
  const checks = Array.from({ length: 500 }, (_, i) => check(`테스트 ✅ ビルド/${i} 🚀`, states[i % states.length]));
  const t = performance.now();
  const sorted = sortChecks(checks);
  const summary = checksSummary(checks);
  assert.ok(performance.now() - t < 50);
  // 500 = 71·7 + 3: indexes 0..2 of the last round add one success, failure and pending.
  assert.equal(summary, "72/500 passed · 214 failing · 72 running · 71 skipped · 71 neutral");
  assert.equal(checksVerdict(checks), "failure");
  const rank = (s: string) => (["failure", "cancelled", "timed_out"].includes(s) ? 0 : s === "pending" ? 1 : 2);
  for (let i = 1; i < sorted.length; i++) {
    const [a, b] = [sorted[i - 1], sorted[i]];
    assert.ok(rank(a.state) < rank(b.state) || (rank(a.state) === rank(b.state) && checks.indexOf(a) < checks.indexOf(b)));
  }
  assert.equal(checks[0].name, "테스트 ✅ ビルド/0 🚀");
});

test("a partial list (checksError) still yields a verdict that can contradict the rollup", () => {
  // read_checks returns the statuses alone when the check runs failed to load; the header then
  // shows their verdict over the rollup's. Documented here, not asserted as right.
  assert.equal(checksVerdict([check("ci/legacy", "success")]), "success");
});
