import assert from "node:assert/strict";
import { test } from "node:test";
import { failureReport } from "./checkFailure.ts";

const URL = "https://github.com/o/r/pull/7";

test("a failure reads as one block: the check, its output, annotations and log", () => {
  const text = failureReport("lint", URL, {
    title: "1 error",
    summary: "",
    annotations: [
      { path: "src/a.ts", line: 3, level: "failure", title: "", message: "'x' is unused" },
      { path: ".github", line: 0, level: "failure", title: "Lint", message: "Process completed with exit code 1." },
    ],
    log: "pnpm lint\n##[error]Process completed with exit code 1.",
    logError: null,
  });
  assert.equal(
    text,
    `CI check "lint" failed on ${URL}\n\n1 error\n\nAnnotations:\nsrc/a.ts:3: failure: 'x' is unused\n.github: failure: Lint: Process completed with exit code 1.\n\nJob log, its last lines up to the error:\npnpm lint\n##[error]Process completed with exit code 1.`,
  );
});

test("what a check doesn't have is left out", () => {
  assert.equal(failureReport("vercel", URL, { title: "", summary: "", annotations: [], log: null, logError: "gone" }), `CI check "vercel" failed on ${URL}`);
});
