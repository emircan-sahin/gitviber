import assert from "node:assert/strict";
import { test } from "node:test";
import { kittyNotes, osc777Note, osc9Note } from "./attention.ts";

test("OSC 9 and 777 notifications, not ConEmu's commands or rxvt's others", () => {
  assert.deepEqual(osc9Note("Claude is waiting for your input"), { body: "Claude is waiting for your input" });
  assert.deepEqual(osc9Note("hello; world"), { body: "hello; world" });
  assert.equal(osc9Note("4;1;50"), null, "the progress bar");
  assert.equal(osc9Note("4;0"), null);
  assert.equal(osc9Note("12"), null);
  assert.deepEqual(osc9Note("42 files changed"), { body: "42 files changed" });
  assert.equal(osc9Note("x".repeat(5000))?.body.length, 1000);
  assert.deepEqual(osc777Note("notify;Claude Code;Needs your permission; to run ls"), { title: "Claude Code", body: "Needs your permission; to run ls" });
  assert.deepEqual(osc777Note("notify;Done"), { title: "Done", body: "" });
  assert.equal(osc777Note("preexec"), null);
});

test("OSC 99 comes in chunks, and a query isn't a notification", () => {
  const read = kittyNotes();
  assert.deepEqual(read(";Hello"), { title: "Hello", body: "" });
  assert.equal(read("i=7:d=0;Claude Code"), null);
  assert.deepEqual(read(`i=7:p=body:e=1;${Buffer.from("Waiting ✅").toString("base64")}`), { title: "Claude Code", body: "Waiting ✅" });
  assert.equal(read("i=1:p=?;"), null);
  assert.equal(read("i=1:p=close;"), null);
  assert.equal(read("i=2:e=1;not base64!"), null);
  assert.equal(read("no payload"), null);
});
