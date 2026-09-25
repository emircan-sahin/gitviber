// `node scripts/updater-json.mjs <dir> <tag> <version> <notes-file>`: writes <dir>/latest.json,
// the file the updater reads from the latest release, from the signed bundles in <dir>.
// The keys follow tauri-plugin-updater: `{os}-{arch}-{bundle}` first, then `{os}-{arch}`.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const [dir, tag, version, notesFile] = process.argv.slice(2);
if (!notesFile) {
  console.error("usage: node scripts/updater-json.mjs <dir> <tag> <version> <notes-file>");
  process.exit(1);
}
const repo = process.env.GITHUB_REPOSITORY ?? "emircan-sahin/gitviber";
const url = (name) => `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(name)}`;

const platforms = {};
const add = (keys, file) => {
  const entry = { signature: readFileSync(path.join(dir, `${file}.sig`), "utf8").trim(), url: url(file) };
  for (const key of keys) platforms[key] = entry;
};

const files = readdirSync(dir);
for (const file of files.filter((f) => files.includes(`${f}.sig`)).sort()) {
  if (file.endsWith("_universal.app.tar.gz")) {
    // One universal build serves both architectures.
    add(["darwin-aarch64", "darwin-x86_64", "darwin-aarch64-app", "darwin-x86_64-app"], file);
    continue;
  }
  const bundle = { ".AppImage": "appimage", ".deb": "deb", ".rpm": "rpm" }[path.extname(file)];
  const arch = /amd64|x86_64/.test(file) ? "x86_64" : /arm64|aarch64/.test(file) ? "aarch64" : null;
  if (!bundle || !arch) continue;
  // Like tauri-action, a Linux install that doesn't find its own bundle type gets the AppImage.
  add(bundle === "appimage" ? [`linux-${arch}`, `linux-${arch}-appimage`] : [`linux-${arch}-${bundle}`], file);
}

// A release that updates only some installs is worse than one that fails here.
const missing = ["darwin-aarch64", "darwin-x86_64", "linux-x86_64"].filter((key) => !platforms[key]);
if (missing.length) {
  console.error(`updater-json: no signed bundle for ${missing.join(", ")}`);
  process.exit(1);
}
const notes = readFileSync(notesFile, "utf8").trim();
const json = { version, notes, pub_date: new Date().toISOString(), platforms };
writeFileSync(path.join(dir, "latest.json"), `${JSON.stringify(json, null, 2)}\n`);
console.log(Object.keys(platforms).sort().join("\n"));
