import assert from "node:assert/strict";
import { test } from "node:test";
import { permissionNotice, testPlan } from "./notifyState.ts";

test("no warning while the switch is off, whatever macOS says", () => {
  for (const s of ["granted", "quiet", "denied", "prompt", "unbundled", null] as const) assert.equal(permissionNotice(false, s), null);
});

test("with the switch on, only a permission that is missing is said, with its fix", () => {
  assert.equal(permissionNotice(true, "granted"), null);
  assert.equal(permissionNotice(true, null), null);
  assert.equal(permissionNotice(true, "denied")?.fix, "settings");
  assert.equal(permissionNotice(true, "quiet")?.fix, "settings");
  assert.equal(permissionNotice(true, "prompt")?.fix, "ask");
  assert.equal(permissionNotice(true, "unbundled")?.fix, null);
});

test("Send Test sends unless macOS would swallow it, and then it names the fix", () => {
  assert.deepEqual(testPlan("granted"), { send: true });
  assert.equal(testPlan("unbundled").send, true);
  assert.equal(testPlan(null).send, true);
  assert.match((testPlan("quiet") as { note: string }).note, /Notification Center/);
  assert.deepEqual(testPlan("denied"), { send: false, title: "macOS isn't allowing GitViber to show notifications", fix: "settings" });
  assert.equal((testPlan("prompt") as { fix: string }).fix, "ask");
});
