// Renders the window screenshot the README banner, the social preview and the link previews show:
// the story's AppWindow (src/components/story) at rest, shot with headless Chrome over the DevTools
// protocol. Run `pnpm capture`, then `pnpm og`, after the UI changes; assets/ is committed, so CI
// never needs Chrome.
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { createServer } from "vite";

const CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const site = path.resolve(import.meta.dirname, "../..");
const assets = path.join(site, "../assets");
const SCALE = 2; // pixels per CSS pixel: the banner and previews are shown at 2x

// name: [chapter, t]. Chapter -1 is the tour of the window, and Infinity is "the whole window at rest".
const STILLS = { "screenshot-dark": [-1, Infinity] };

const server = await createServer({ root: site, server: { port: 4399, strictPort: true }, logLevel: "error" });
await server.listen();
const base = "http://localhost:4399/gitviber";

const profile = mkdtempSync(path.join(tmpdir(), "gv-capture-"));
const chrome = spawn(CHROME, ["--headless=new", "--remote-debugging-port=9339", `--user-data-dir=${profile}`, "--hide-scrollbars", "--force-device-scale-factor=1", "--window-size=1400,900", "about:blank"], { stdio: "ignore" });
const done = async () => {
  chrome.kill();
  await new Promise((r) => (chrome.exitCode === null ? chrome.once("exit", r) : r()));
  rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  await server.close();
};

try {
  let target;
  for (let i = 0; i < 50 && !target; i++) {
    try {
      target = await (await fetch("http://127.0.0.1:9339/json/new?" + `${base}/capture.html`, { method: "PUT" })).json();
    } catch {
      await new Promise((r) => setTimeout(r, 200));
    }
  }
  if (!target) throw new Error("capture: Chrome did not start");
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((r) => (ws.onopen = r));
  let id = 0;
  const waiting = new Map();
  ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    waiting.get(m.id)?.(m);
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const n = ++id;
      waiting.set(n, (m) => (m.error ? reject(new Error(m.error.message)) : resolve(m.result)));
      ws.send(JSON.stringify({ id: n, method, params }));
    });
  const evaluate = async (expression) => {
    const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? "evaluate failed");
    return r.result.value;
  };

  await send("Page.enable");
  await send("Emulation.setDeviceMetricsOverride", { width: 1340, height: 780, deviceScaleFactor: 1, mobile: false });
  for (let i = 0; i < 100 && !(await evaluate("typeof window.__frame === 'function'")); i++) await new Promise((r) => setTimeout(r, 200));
  await evaluate("document.fonts.ready.then(() => true)");

  for (const [name, [chapter, t]] of Object.entries(STILLS)) {
    await evaluate(`window.__frame(${chapter}, ${t})`);
    // Fonts and layout settle within a frame or two.
    await evaluate("new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))");
    const { data } = await send("Page.captureScreenshot", { format: "png", clip: { x: 0, y: 0, width: 1340, height: 780, scale: SCALE } });
    const out = path.join(assets, `${name}.png`);
    writeFileSync(out, Buffer.from(data, "base64"));
    console.log(`${name}: ${(statSync(out).size / 1024).toFixed(0)} KB`);
  }
  ws.close();
} finally {
  await done();
}
