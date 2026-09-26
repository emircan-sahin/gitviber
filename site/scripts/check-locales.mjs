// Fails the build when a translation doesn't match en.json: a missing or extra key, an array of
// another length, or a string whose {placeholders} or `code` spans differ from the English one.
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

const dir = path.resolve(import.meta.dirname, "../src/i18n/messages");
const load = (file) => JSON.parse(readFileSync(path.join(dir, file), "utf8"));
const en = load("en.json");

const marks = (s) => [...(s.match(/\{\w+\}/g) ?? []), ...(s.match(/`[^`]+`/g) ?? [])].sort().join(" ");

function compare(src, dst, at, errors) {
  if (typeof src === "string") {
    if (typeof dst !== "string") return errors.push(`${at}: expected a string`);
    if (!dst.trim()) errors.push(`${at}: empty`);
    if (marks(src) !== marks(dst)) errors.push(`${at}: placeholders/code differ (${marks(src)} vs ${marks(dst)})`);
    return;
  }
  if (Array.isArray(src)) {
    if (!Array.isArray(dst) || dst.length !== src.length) return errors.push(`${at}: expected ${src.length} items`);
    src.forEach((v, i) => compare(v, dst[i], `${at}[${i}]`, errors));
    return;
  }
  if (typeof dst !== "object" || dst === null || Array.isArray(dst)) return errors.push(`${at}: expected an object`);
  for (const key of Object.keys(src)) {
    if (!(key in dst)) errors.push(`${at}.${key}: missing`);
    else compare(src[key], dst[key], `${at}.${key}`, errors);
  }
  for (const key of Object.keys(dst)) if (!(key in src)) errors.push(`${at}.${key}: not in en.json`);
}

let failed = false;
for (const file of readdirSync(dir).filter((f) => f.endsWith(".json") && f !== "en.json")) {
  const errors = [];
  compare(en, load(file), file.replace(".json", ""), errors);
  if (errors.length) {
    failed = true;
    console.error(errors.join("\n"));
  }
}
if (failed) process.exit(1);
