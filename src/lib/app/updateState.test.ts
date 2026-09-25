import assert from "node:assert/strict";
import { test } from "node:test";
import { checked, downloaded, due, INITIAL, isExpectedFailure, percent, type Release, type UpdateState } from "./updateState.ts";

const v2: Release = { version: "0.2.0", notes: "- Faster diffs", date: null };
const v3: Release = { version: "0.3.0", notes: "", date: null };

test("a check shows what it found, and an empty answer clears it", () => {
  const found = checked({ ...INITIAL, checking: true }, v2, 1000);
  assert.deepEqual(found, { release: v2, download: null, checking: false, checkedAt: 1000 });
  assert.equal(checked(found, null, 2000).release, null);
  assert.equal(checked(found, v3, 2000).release, v3);
});

test("a release being downloaded or waiting for a restart outlives newer checks", () => {
  const downloading: UpdateState = { ...INITIAL, release: v2, download: { received: 10, total: 100 } };
  assert.equal(checked(downloading, v3, 1).release, v2);
  assert.equal(checked({ ...downloading, download: "ready" }, null, 1).release, v2);
});

test("download progress adds up; Finished leaves ready to the resolved download", () => {
  let s: UpdateState = { ...INITIAL, release: v2, download: { received: 0, total: null } };
  s = downloaded(s, { event: "Started", data: { contentLength: 200 } });
  s = downloaded(s, { event: "Progress", data: { chunkLength: 50 } });
  s = downloaded(s, { event: "Progress", data: { chunkLength: 49 } });
  assert.deepEqual(s.download, { received: 99, total: 200 });
  assert.equal(downloaded(s, { event: "Finished" }), s);
  assert.deepEqual(downloaded(s, { event: "Started", data: {} }).download, { received: 0, total: null });
});

test("percent rounds down, caps at 100 and needs a size", () => {
  assert.equal(percent({ received: 99, total: 200 }), 49);
  assert.equal(percent({ received: 300, total: 200 }), 100);
  assert.equal(percent({ received: 5, total: null }), null);
  assert.equal(percent({ received: 5, total: 0 }), null);
});

test("a check is due when never done or old enough", () => {
  assert.equal(due(null, 0, 100), true);
  assert.equal(due(1000, 1099, 100), false);
  assert.equal(due(1000, 1100, 100), true);
});

test("offline and no release yet stay quiet; broken release data doesn't", () => {
  assert.equal(isExpectedFailure("error sending request for url (https://github.com/emircan-sahin/gitviber/releases/latest/download/latest.json)"), true);
  assert.equal(isExpectedFailure("Could not fetch a valid release JSON from the remote"), true);
  assert.equal(isExpectedFailure("None of the fallback platforms `[\"linux-aarch64\"]` were found in the response `platforms` object"), false);
  assert.equal(isExpectedFailure("missing field `version`"), false);
});
