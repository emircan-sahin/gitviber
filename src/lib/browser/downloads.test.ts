import assert from "node:assert/strict";
import { test } from "node:test";
import { type BrowserDownload, downloadText, folderName, reported } from "./downloads.ts";

const d = (key: number, over: Partial<BrowserDownload> = {}): BrowserDownload => ({ tab: "t", key, name: `f${key}.zip`, state: "going", received: 0, total: -1, path: null, why: null, ...over });

test("a report changes the download of its key and adds a new one", () => {
  const list = reported(reported([], d(1)), d(2));
  assert.deepEqual(
    reported(list, d(1, { received: 5 })).map((x) => [x.key, x.received]),
    [
      [1, 5],
      [2, 0],
    ],
  );
});

test("a tab keeps its latest few, ended ones going first and those going never", () => {
  let list: BrowserDownload[] = [];
  for (let k = 1; k <= 4; k++) list = reported(list, d(k, { state: k === 1 ? "going" : "done" }));
  list = reported(list, d(5));
  assert.deepEqual(
    list.map((x) => x.key),
    [1, 3, 4, 5],
  );
  const busy = [1, 2, 3, 4].map((k) => d(k));
  assert.equal(reported(busy, d(5)).length, 5, "none going is dropped");
});

test("the text says how far it is, where it went or why not", () => {
  assert.equal(downloadText(d(1)), "Starting");
  assert.equal(downloadText(d(1, { received: 2048 })), "2.0 KB");
  assert.equal(downloadText(d(1, { received: 524288, total: 1048576 })), "50% of 1.0 MB");
  assert.equal(downloadText(d(1, { received: 9, total: 8 })), "100% of 8 B");
  assert.equal(downloadText(d(1, { state: "done", path: "/Users/me/Downloads/a.zip" })), "Saved to Downloads");
  assert.equal(downloadText(d(1, { state: "cancelled" })), "Cancelled");
  assert.equal(downloadText(d(1, { state: "failed", why: "the network dropped" })), "Failed: the network dropped");
  assert.equal(folderName("/a.zip"), "Downloads");
});
