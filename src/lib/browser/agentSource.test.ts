import { readFileSync } from "node:fs";

// The agent script as agent.rs joins it (AGENT's include_str! list, in its order), for the tests
// that run it: one list, so they run what the app runs. Its own test is agentParts.test.ts.
export const browser = new URL("../../../src-tauri/src/browser/", import.meta.url);
const rust = readFileSync(new URL("agent.rs", browser), "utf8");
const joined = /const AGENT: &str = concat!\(([\s\S]*?)\);/.exec(rust)?.[1] ?? "";
export const parts = [...joined.matchAll(/include_str!\("(scripts\/agent\/[\w.]+)"\)/g)].map((m) => m[1]);
export const source = parts.map((p) => readFileSync(new URL(p, browser), "utf8")).join("");
