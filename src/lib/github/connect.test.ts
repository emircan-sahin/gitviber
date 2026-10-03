import assert from "node:assert/strict";
import { test } from "node:test";
import { remoteKind, signedOutNote } from "./connect.ts";

test("remotes are told apart by how git signs in to them", () => {
  assert.equal(remoteKind("git@github.com:octo-one/demo.git"), "ssh");
  assert.equal(remoteKind("ssh://git@github.com/octo-one/demo.git"), "ssh");
  assert.equal(remoteKind("https://github.com/octo-one/demo.git"), "https");
  assert.equal(remoteKind("HTTP://github.com/octo-one/demo"), "https");
  assert.equal(remoteKind("/Users/someone/demo.git"), "other");
  assert.equal(remoteKind("C:\\work\\demo.git"), "other");
  assert.equal(remoteKind(null), "other");
  assert.equal(remoteKind(undefined), "other");
});

test("an SSH remote is told its key does not reach the API", () => {
  const ssh = signedOutNote("git@github.com:octo-one/demo.git", "pull requests");
  assert.match(ssh, /uses SSH/);
  assert.match(ssh, /^Your remote.*Pull requests come from GitHub's API/);
  assert.match(signedOutNote("ssh://git@github.com/o/r", "issues"), /Issues come from/);
  for (const url of ["https://github.com/o/r", null, undefined]) {
    assert.doesNotMatch(signedOutNote(url, "issues"), /SSH/);
  }
});
