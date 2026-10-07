import assert from "node:assert/strict";
import { test } from "node:test";
import { commitPlace, failureReport } from "./checkFailure.ts";

const URL = "https://github.com/o/r/pull/7";

test("a failure reads as one block: the check, its output, annotations and log", () => {
  const text = failureReport("lint", URL, {
    title: "1 error",
    summary: "",
    annotations: [
      { path: "src/a.ts", line: 3, level: "failure", title: "", message: "'x' is unused" },
      { path: ".github", line: 0, level: "failure", title: "Lint", message: "Process completed with exit code 1." },
    ],
    annotationsError: null,
    log: "pnpm lint\n##[error]Process completed with exit code 1.",
    logError: null,
  });
  assert.equal(
    text,
    `CI check "lint" failed on ${URL}\n\n1 error\n\nAnnotations:\nsrc/a.ts:3: failure: 'x' is unused\n.github: failure: Lint: Process completed with exit code 1.\n\nJob log, its last lines up to the error:\npnpm lint\n##[error]Process completed with exit code 1.`,
  );
});

test("what a check doesn't have is left out", () => {
  assert.equal(failureReport("vercel", URL, { title: "", summary: "", annotations: [], annotationsError: null, log: null, logError: "gone" }), `CI check "vercel" failed on ${URL}`);
});

test("a commit's failure names the commit, by id and subject", () => {
  const where = commitPlace("1a2b3c4", "Fix the parser", "https://github.com/acme/widgets/commit/1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b");
  assert.equal(
    failureReport("test", where, { title: "", summary: "", annotations: [], annotationsError: null, log: "boom", logError: null }),
    'CI check "test" failed on commit 1a2b3c4 "Fix the parser" (https://github.com/acme/widgets/commit/1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b)\n\nJob log, its last lines up to the error:\nboom',
  );
});
