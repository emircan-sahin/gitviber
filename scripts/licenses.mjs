// Writes src/lib/third-party-licenses.json: every npm and cargo package the app ships, with
// the license files each one includes (MIT, Apache-2.0 and OFL require passing them on).
// Shown under About → Third-Party Licenses. `--check` fails when the file is out of date,
// so `pnpm check` catches a dependency change that wasn't followed by
// `pnpm licenses:generate`.
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, realpathSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const out = path.join(root, "src/lib/third-party-licenses.json");
const LICENSE_FILE = /^(licen[cs]e|copying|notice|unlicense)([-._].*)?$/i;

const licenseTexts = (dir, extra = []) => {
  const names = readdirSync(dir).filter((f) => LICENSE_FILE.test(f) && statSync(path.join(dir, f)).isFile());
  const files = [...new Set([...names.map((f) => path.join(dir, f)), ...extra])].sort();
  return files.map((f) => readFileSync(f, "utf8").replace(/\r\n?/g, "\n").trim()).filter(Boolean);
};

// Production dependencies, followed through node_modules the way Node resolves them.
function npmPackages() {
  const found = new Map();
  const resolve = (from, name) => {
    for (let dir = from; ; dir = path.dirname(dir)) {
      const candidate = path.join(dir, "node_modules", name);
      if (existsSync(path.join(candidate, "package.json"))) return realpathSync(candidate);
      if (dir === path.dirname(dir)) return null;
    }
  };
  const visit = (dir, isRoot) => {
    const pkg = JSON.parse(readFileSync(path.join(dir, "package.json"), "utf8"));
    const key = `${pkg.name}@${pkg.version}`;
    if (!isRoot) {
      if (found.has(key)) return;
      const license = typeof pkg.license === "string" ? pkg.license : (pkg.license?.type ?? pkg.licenses?.map((l) => l.type).join(" OR "));
      const repo = typeof pkg.repository === "string" ? pkg.repository : pkg.repository?.url;
      found.set(key, { name: pkg.name, version: pkg.version, license: license ?? "UNKNOWN", url: npmUrl(pkg.homepage ?? repo, pkg.name), texts: licenseTexts(dir) });
    }
    // Optional dependencies are platform-specific, so they're left out to keep the file the
    // same on every machine; none of the runtime packages have one today.
    for (const name of Object.keys(pkg.dependencies ?? {})) {
      const dep = resolve(dir, name);
      if (!dep) throw new Error(`${name} (needed by ${key}) isn't installed; run pnpm install`);
      visit(dep, false);
    }
  };
  visit(root, true);
  return [...found.values()];
}

const npmUrl = (url, name) => {
  if (!url) return `https://www.npmjs.com/package/${name}`;
  return url
    .replace(/^git\+/, "")
    .replace(/^git:\/\//, "https://")
    .replace(/^github:/, "https://github.com/")
    .replace(/^ssh:\/\/git@/, "https://")
    .replace(/\.git(#.*)?$/, "");
};

// Normal (non-build, non-dev) dependencies of the app crate, on every target platform.
function cargoPackages() {
  const json = execFileSync("cargo", ["metadata", "--format-version", "1", "--locked", "--manifest-path", path.join(root, "src-tauri/Cargo.toml")], {
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
  const meta = JSON.parse(json);
  const byId = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const seen = new Set();
  const queue = [meta.resolve.root];
  while (queue.length) {
    const node = nodes.get(queue.pop());
    for (const dep of node.deps) {
      if (seen.has(dep.pkg) || !dep.dep_kinds.some((k) => k.kind === null)) continue;
      seen.add(dep.pkg);
      queue.push(dep.pkg);
    }
  }
  return [...seen].map((id) => {
    const p = byId.get(id);
    const dir = path.dirname(p.manifest_path);
    const extra = p.license_file ? [path.resolve(dir, p.license_file)] : [];
    return {
      name: p.name,
      version: p.version,
      license: p.license ?? (p.license_file ? "See license text" : "UNKNOWN"),
      url: p.repository?.replace(/\.git$/, "") ?? p.homepage ?? `https://crates.io/crates/${p.name}`,
      texts: licenseTexts(dir, extra),
    };
  });
}

// A package that ships no license file (a crate whose license sits at its workspace root,
// say) gets the standard text of the license it offers, MIT when it's one of the choices.
function withStandardText(p) {
  if (p.texts.length) return p;
  const ids = p.license.split(/\s+OR\s+|\s*\/\s*/).map((id) => id.replace(/[()]/g, "").trim());
  const id = ids.includes("MIT") ? "MIT" : ids[0];
  const file = path.join(root, "scripts/license-texts", `${id}.txt`);
  if (/\sAND\s/.test(p.license) || !existsSync(file)) {
    console.warn(`${p.name}@${p.version} (${p.license}) ships no license file and there's no standard text for it in scripts/license-texts/`);
    return p;
  }
  return { ...p, standard: id, texts: [readFileSync(file, "utf8").trim()] };
}

// Plain code-unit order: localeCompare depends on the ICU data Node was built with.
const cmp = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

// Identical texts (the Apache-2.0 body, most MIT files) are stored once and referenced.
function build() {
  const texts = [];
  const index = new Map();
  const ref = (t) => {
    if (!index.has(t)) index.set(t, texts.push(t) - 1);
    return index.get(t);
  };
  const packages = [...npmPackages().map((p) => ({ ...p, source: "npm" })), ...cargoPackages().map((p) => ({ ...p, source: "cargo" }))]
    .map(withStandardText)
    .sort((a, b) => cmp(a.name, b.name) || cmp(a.source, b.source) || cmp(a.version, b.version))
    .map(({ texts: t, ...p }) => ({ ...p, texts: t.map(ref) }));
  return `${JSON.stringify({ packages, texts }, null, 1)}\n`;
}

const next = build();
if (process.argv.includes("--check")) {
  // A Windows checkout with core.autocrlf has CRLF line endings.
  const current = existsSync(out) ? readFileSync(out, "utf8").replace(/\r\n/g, "\n") : "";
  if (current !== next) {
    console.error("src/lib/third-party-licenses.json is out of date; run `pnpm licenses:generate` and commit it.");
    process.exit(1);
  }
} else {
  writeFileSync(out, next);
  const { packages } = JSON.parse(next);
  console.log(`${packages.length} packages, ${packages.filter((p) => p.standard).length} with a standard license text`);
}
