// `node scripts/release-notes.mjs vX.Y.Z`: prints X.Y.Z's CHANGELOG.md section, which becomes
// the GitHub release's notes and the updater's. Fails when the section is missing, empty or
// undated, so a tag can't ship with notes that weren't written. A test tag (vX.Y.Z-rc.1)
// uses X.Y.Z's section, dated or not.
import { readFileSync } from "node:fs";
import path from "node:path";

const tag = process.argv[2] ?? "";
const fail = (message) => {
  console.error(`release-notes: ${message}`);
  process.exit(1);
};
const match = tag.match(/^v(\d+\.\d+\.\d+)(-.+)?$/);
if (!match) fail("usage: node scripts/release-notes.mjs vX.Y.Z");
const [, version, prerelease] = match;

const changelog = readFileSync(path.resolve(import.meta.dirname, "../CHANGELOG.md"), "utf8");
const lines = changelog.split("\n");
const start = lines.findIndex((l) => l.startsWith(`## [${version}]`));
if (start < 0) fail(`CHANGELOG.md has no "## [${version}]" section`);
if (!prerelease && !/ - \d{4}-\d{2}-\d{2}$/.test(lines[start])) {
  fail(`date the heading: "## [${version}] - YYYY-MM-DD", not "${lines[start]}"`);
}
// The section runs to the next heading of its level or the link references at the bottom.
const end = lines.findIndex((l, i) => i > start && (l.startsWith("## ") || /^\[[^\]]+\]: /.test(l)));
const notes = lines.slice(start + 1, end < 0 ? undefined : end).join("\n").trim();
if (!notes) fail(`the ${version} section of CHANGELOG.md is empty`);
console.log(notes);
