import assert from "node:assert/strict";
import { test } from "node:test";
import { osc52Text } from "./osc52.ts";

test("OSC 52 copies text, and never answers a read", () => {
  assert.equal(osc52Text(`c;${Buffer.from("héllo ✅\n").toString("base64")}`), "héllo ✅\n");
  assert.equal(osc52Text(`;${btoa("tmux")}`), "tmux");
  assert.equal(osc52Text("c;?"), null);
  assert.equal(osc52Text("c;"), null);
  assert.equal(osc52Text("c"), null);
  assert.equal(osc52Text("c;not base64!"), null);
  // Not UTF-8.
  assert.equal(osc52Text(`c;${btoa("\xff\xfe")}`), null);
});
