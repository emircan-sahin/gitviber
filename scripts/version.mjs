// The app version lives in package.json; tauri.conf.json points at it. Cargo can't read it
// from there, so Cargo.toml and Cargo.lock carry a copy that has to match.
//   node scripts/version.mjs check [tag]      fails on a mismatch, or when the tag isn't
//                                             vX.Y.Z or a test tag like vX.Y.Z-rc.1
//   node scripts/version.mjs set X.Y.Z        writes the version to all three files
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const file = (rel) => path.join(root, rel);
const read = (rel) => readFileSync(file(rel), "utf8");
const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;

// Each copy of the version: where it is, and a pattern whose second group is the version.
const sources = [
  { rel: "package.json", re: /^(\s*"version":\s*")([^"]+)"/m },
  { rel: "src-tauri/Cargo.toml", re: /^(\[package\][^[]*?\nversion\s*=\s*")([^"]+)"/m },
  { rel: "src-tauri/Cargo.lock", re: /^(name = "gitviber"\nversion = ")([^"]+)"/m },
];

const fail = (message) => {
  console.error(`version: ${message}`);
  process.exit(1);
};

function versions() {
  return sources.map(({ rel, re }) => {
    const match = read(rel).match(re);
    if (!match) fail(`no version found in ${rel}`);
    return { rel, version: match[2] };
  });
}

function check(tag) {
  const conf = JSON.parse(read("src-tauri/tauri.conf.json"));
  if (conf.version !== "../package.json") {
    fail(`src-tauri/tauri.conf.json "version" must be "../package.json", not ${JSON.stringify(conf.version)}`);
  }
  const found = versions();
  const [{ version }] = found;
  if (!SEMVER.test(version)) fail(`package.json version ${version} isn't semver (X.Y.Z)`);
  const off = found.filter((f) => f.version !== version);
  if (off.length) {
    fail(`${off.map((f) => `${f.rel} has ${f.version}`).join(", ")}, package.json has ${version}. Run pnpm version:set ${version}`);
  }
  if (tag !== undefined && tag !== `v${version}` && !tag.startsWith(`v${version}-`)) {
    fail(`tag ${tag} doesn't match the version, v${version}`);
  }
  console.log(version);
}

function set(version) {
  if (!SEMVER.test(version ?? "")) fail("usage: pnpm version:set X.Y.Z");
  for (const { rel, re } of sources) {
    writeFileSync(file(rel), read(rel).replace(re, `$1${version}"`));
  }
  check();
}

const [command, arg] = process.argv.slice(2);
if (command === "check") check(arg);
else if (command === "set") set(arg);
else fail("usage: node scripts/version.mjs check [tag] | set X.Y.Z");
