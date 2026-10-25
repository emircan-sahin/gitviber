import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { test } from "node:test";
import { browser, parts, source } from "./agentSource.ts";

test("the agent script is every part of scripts/agent, the prelude first and the run last", () => {
  const files = readdirSync(new URL("scripts/agent/", browser)).filter((f) => f.endsWith(".js"));
  assert.deepEqual([...parts].sort(), files.map((f) => `scripts/agent/${f}`).sort());
  assert.equal(parts[0], "scripts/agent/prelude.js");
  assert.equal(parts.at(-1), "scripts/agent/run.js");
  // One function body, as WebKit runs it.
  const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
  assert.doesNotThrow(() => new AsyncFunction("input", source));
});
