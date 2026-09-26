// Renders card.html with headless Chrome (macOS) into the site's icons, one link preview per
// locale (public/og/<code>.png), the repo's social preview and the README banner. Rerun it after a
// new translation, a copy change in messages or a new screenshot; the PNGs are committed, so CI
// never needs Chrome.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

const CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const site = path.resolve(import.meta.dirname, "../..");
const pub = path.join(site, "public");
const assets = path.join(site, "../assets");
const messagesDir = path.join(site, "src/i18n/messages");
const MAX_BYTES = 1024 * 1024;

// assets/icon.svg is the macOS icon: a rounded square with a margin around it. Tabs and home
// screens want the square without the margin; full-bleed for masks that round it themselves.
const iconSvg = readFileSync(path.join(assets, "icon.svg"), "utf8");
function variant(svg, { crop, square }) {
  let out = svg;
  const swap = (from, to) => {
    if (!out.includes(from)) throw new Error(`og: assets/icon.svg has no ${from}; update render.mjs`);
    out = out.replace(from, to);
  };
  if (crop) swap('width="1024" height="1024" viewBox="0 0 1024 1024"', 'viewBox="100 100 824 824"');
  if (square) {
    swap('<rect x="100" y="100" width="824" height="824" rx="190" fill="url(#bg)"/>', `<rect ${crop ? 'x="100" y="100" width="824" height="824"' : 'width="1024" height="1024"'} fill="url(#bg)"/>`);
    out = out.replace(/\s*<rect [^>]*stroke="#3a3731"[^>]*\/>/, "");
  }
  return out;
}
const favicon = variant(iconSvg, { crop: true });
const dataUrl = (svg) => `data:image/svg+xml;base64,${Buffer.from(svg).toString("base64")}`;

/** A one-image .ico holding a PNG, which every current browser reads. */
function ico(png) {
  const head = Buffer.alloc(22);
  head.writeUInt16LE(1, 2); // type: icon
  head.writeUInt16LE(1, 4); // one image
  head.writeUInt8(32, 6); // width
  head.writeUInt8(32, 7); // height
  head.writeUInt16LE(1, 10); // color planes
  head.writeUInt16LE(32, 12); // bits per pixel
  head.writeUInt32LE(png.length, 14);
  head.writeUInt32LE(22, 18); // offset of the PNG
  return Buffer.concat([head, png]);
}

// A minimal DevTools protocol client over the WebSocket Chrome opens.
async function launch() {
  const profile = mkdtempSync(path.join(tmpdir(), "gitviber-og-"));
  const chrome = spawn(
    CHROME,
    ["--headless=new", "--disable-gpu", "--hide-scrollbars", "--no-first-run", "--allow-file-access-from-files", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank"],
    { stdio: "ignore" },
  );
  const exited = new Promise((resolve) => chrome.once("exit", resolve));
  // Chrome keeps writing to its profile until it's gone, so wait for the exit before removing it.
  const close = async () => {
    chrome.kill("SIGKILL");
    await exited;
    rmSync(profile, { recursive: true, force: true, maxRetries: 5 });
  };
  process.on("exit", () => {
    chrome.kill("SIGKILL");
    try {
      rmSync(profile, { recursive: true, force: true, maxRetries: 5 });
    } catch {}
  });
  for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => process.exit(1));

  const portFile = path.join(profile, "DevToolsActivePort");
  for (let i = 0; i < 100 && !existsSync(portFile); i++) await new Promise((r) => setTimeout(r, 100));
  if (!existsSync(portFile)) throw new Error("og: Chrome didn't start");
  const [port, wsPath] = readFileSync(portFile, "utf8").trim().split("\n");
  const ws = new WebSocket(`ws://127.0.0.1:${port}${wsPath}`);
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = reject;
  });

  let id = 0;
  const pending = new Map();
  const waiters = [];
  ws.onmessage = ({ data }) => {
    const msg = JSON.parse(data);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
    } else if (msg.method) {
      for (const w of waiters.filter((w) => w.method === msg.method)) {
        waiters.splice(waiters.indexOf(w), 1);
        w.resolve(msg.params);
      }
    }
  };
  const send = (method, params = {}, sessionId) =>
    new Promise((resolve, reject) => {
      pending.set(++id, { resolve, reject });
      ws.send(JSON.stringify({ id, method, params, sessionId }));
    });
  const once = (method) => new Promise((resolve) => waiters.push({ method, resolve }));

  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  const page = (method, params) => send(method, params, sessionId);
  await page("Page.enable");
  await page("Runtime.enable");
  const loaded = once("Page.loadEventFired");
  await page("Page.navigate", { url: pathToFileURL(path.join(import.meta.dirname, "card.html")).href });
  await loaded;

  return { page, close };
}

async function shoot(page, out, { width, height, scale = 1, ...data }) {
  await page("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: scale, mobile: false });
  await page("Emulation.setDefaultBackgroundColorOverride", { color: { r: 0, g: 0, b: 0, a: 0 } });
  const { exceptionDetails } = await page("Runtime.evaluate", {
    expression: `render(${JSON.stringify({ width, height, ...data })})`,
    awaitPromise: true,
  });
  if (exceptionDetails) throw new Error(`og: ${exceptionDetails.exception?.description ?? exceptionDetails.text}`);
  const { data: png } = await page("Page.captureScreenshot", { format: "png", clip: { x: 0, y: 0, width, height, scale: 1 } });
  const buf = Buffer.from(png, "base64");
  if (out) {
    mkdirSync(path.dirname(out), { recursive: true });
    writeFileSync(out, buf);
    const size = statSync(out).size;
    console.log(`wrote ${path.relative(site, out)} (${Math.round(size / 1024)} KB)`);
    if (size > MAX_BYTES) throw new Error(`og: ${out} is over 1 MB`);
  }
  return buf;
}

const { page, close } = await launch();
try {
  // Icons. favicon.svg is the tab icon; the PNGs are for home screens, the manifest and Safari.
  writeFileSync(path.join(pub, "favicon.svg"), favicon);
  console.log("wrote public/favicon.svg");
  const rounded = dataUrl(favicon);
  const fullBleed = dataUrl(variant(iconSvg, { crop: true, square: true }));
  const maskable = dataUrl(variant(iconSvg, { crop: false, square: true }));
  const png32 = await shoot(page, null, { kind: "icon", width: 32, height: 32, icon: rounded });
  writeFileSync(path.join(pub, "favicon.ico"), ico(png32));
  console.log("wrote public/favicon.ico");
  await shoot(page, path.join(pub, "apple-touch-icon.png"), { kind: "icon", width: 180, height: 180, icon: fullBleed });
  await shoot(page, path.join(pub, "icon-192.png"), { kind: "icon", width: 192, height: 192, icon: rounded });
  await shoot(page, path.join(pub, "icon-512.png"), { kind: "icon", width: 512, height: 512, icon: rounded });
  await shoot(page, path.join(pub, "icon-maskable-512.png"), { kind: "icon", width: 512, height: 512, icon: maskable });

  // Relative to card.html.
  const common = { icon: "../../public/favicon.svg", screenshot: "../../../assets/screenshot-dark.png" };
  const copy = (code) => {
    const t = JSON.parse(readFileSync(path.join(messagesDir, `${code}.json`), "utf8"));
    return { lang: code, headline: t.hero.title, tagline: t.og.tagline, facts: t.og.facts };
  };

  // One link preview per translation, named like head.ts expects (ogImage).
  for (const file of readdirSync(messagesDir).filter((f) => f.endsWith(".json"))) {
    const code = file.replace(/\.json$/, "");
    const out = path.join(pub, "og", `${code.toLowerCase()}.png`);
    await shoot(page, out, { kind: "hero", width: 1200, height: 630, stageTop: 44, ...common, ...copy(code) });
  }

  // Uploaded by hand in the repo's Settings → Social preview, and shown at the top of the README.
  const en = copy("en");
  // At 2x: GitHub shows it past 1280px wide on retina screens, and at 1x it was visibly soft.
  await shoot(page, path.join(assets, "social-preview.png"), { kind: "hero", width: 1280, height: 640, stageTop: 48, scale: 2, ...common, ...en });
  await shoot(page, path.join(assets, "banner.png"), { kind: "banner", width: 1280, height: 400, scale: 2, ...common, ...en });
} finally {
  await close();
}
process.exit(0);
