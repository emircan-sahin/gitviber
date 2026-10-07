import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// translucency.ts and settings.ts import "../api" and "./api" the bundler way (no extension, a
// folder): let node find the .ts.
// Each boot's ?boot= query is passed down to our own modules, so platform.ts is read afresh too.
register(
  "data:text/javascript," +
    encodeURIComponent(`
async function find(spec, ctx, next) {
  try {
    return await next(spec, ctx);
  } catch (e) {
    if (!spec.startsWith(".")) throw e;
    for (const tail of [".ts", "/index.ts"]) {
      try {
        return await next(spec + tail, ctx);
      } catch {}
    }
    throw e;
  }
}
export async function resolve(spec, ctx, next) {
  const found = await find(spec, ctx, next);
  const boot = ctx.parentURL && new URL(ctx.parentURL).search;
  if (boot && spec.startsWith(".") && !found.url.includes("?")) return { ...found, url: found.url + boot };
  return found;
}`),
);

type Settings = typeof import("../settings.ts");
type Pending = { on: boolean; resolve: () => void; reject: (e: unknown) => void };

/**
 * A fresh translucency.ts (and settings.ts) in a fake macOS window: set_translucent calls wait in `pending` (in the
 * order the main thread would run them) until drained, as a busy main thread would hold them.
 */
async function boot({ stored, platform = "MacIntel", focused = false, reduce = false }: { stored?: object; platform?: string; focused?: boolean; reduce?: boolean | (() => Promise<boolean>) } = {}) {
  const g = globalThis as Record<string, any>;
  Object.defineProperty(g, "navigator", { value: { platform, userAgent: "" }, configurable: true, writable: true });
  const store = new Map<string, string>();
  if (stored) store.set("gitviber.settings.v2", JSON.stringify(stored));
  g.localStorage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => store.set(k, v) };
  const root = { dataset: {} as Record<string, string>, style: { setProperty() {} } };
  g.document = { documentElement: root };
  g.window = g;
  g.location = { search: "" };
  g.matchMedia = () => ({ matches: true, addEventListener() {} });
  let frames: (() => void)[] = [];
  g.requestAnimationFrame = (cb: () => void) => frames.push(cb);

  const pending: Pending[] = [];
  const asked: boolean[] = [];
  let native = false;
  const callbacks = new Map<number, (e: unknown) => void>();
  const listeners = new Map<string, number[]>();
  let nextId = 1;
  g.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    transformCallback: (cb: (e: unknown) => void) => {
      callbacks.set(nextId, cb);
      return nextId++;
    },
    invoke: async (cmd: string, args: Record<string, any>) => {
      if (cmd === "set_translucent") {
        asked.push(args.on);
        return new Promise<void>((resolve, reject) =>
          pending.push({
            on: args.on,
            resolve: () => {
              native = args.on;
              resolve();
            },
            reject,
          }),
        );
      }
      if (cmd === "reduce_transparency") return typeof reduce === "function" ? reduce() : reduce;
      if (cmd === "plugin:window|is_focused") return focused;
      if (cmd === "plugin:event|listen") {
        listeners.set(args.event, [...(listeners.get(args.event) ?? []), args.handler]);
        return args.handler;
      }
      return null;
    },
  };

  const query = `?boot=${Math.random()}`;
  await import(`./translucency.ts${query}`);
  // The same URL as translucency.ts's own import: the same module.
  const settings: Settings = await import(`../settings.ts${query}`);
  await settle();

  async function settle() {
    for (let i = 0; i < 20; i++) await new Promise((r) => setImmediate(r));
  }
  return {
    settings,
    root,
    asked,
    pending,
    settle,
    get native() {
      return native;
    },
    /** The page's look: the translucency level it shows, or undefined when solid. */
    get look() {
      return root.dataset.translucency;
    },
    /** Runs the main thread's queued set_translucent calls, in order, and the ones they lead to. */
    async drain() {
      // Turning it on asks Reduce transparency first, so the call can still be on its way.
      await settle();
      while (pending.length) {
        while (pending.length) pending.shift()!.resolve();
        await settle();
      }
    },
    async frame(n = 1) {
      for (let i = 0; i < n; i++) {
        const run = frames;
        frames = [];
        run.forEach((cb) => cb());
        await settle();
      }
    },
    async focus(on: boolean) {
      for (const id of listeners.get(on ? "tauri://focus" : "tauri://blur") ?? []) callbacks.get(id)!({ event: "", id: 0, payload: null });
      await settle();
    },
  };
}

test("off, macOS asks nothing of the window and the page stays as it was", async () => {
  const w = await boot({ focused: true });
  await w.focus(false);
  await w.focus(true);
  w.settings.updateSettings({ appearance: "light" });
  await w.frame(3);
  assert.equal(w.settings.getSettings().translucency, "off");
  assert.deepEqual(w.asked, []);
  assert.equal(w.look, undefined);
  assert.equal("translucency" in w.root.dataset, false);
});

test("Reduce transparency is asked only while it's on: once when turned on, then on each return", async () => {
  let asks = 0;
  const w = await boot({
    focused: true,
    reduce: async () => {
      asks++;
      return false;
    },
  });
  await w.focus(false);
  await w.focus(true);
  assert.equal(asks, 0, "off");
  w.settings.updateSettings({ translucency: "subtle" });
  await w.drain();
  assert.equal(asks, 1);
  w.settings.updateSettings({ translucency: "strong" });
  await w.focus(false);
  await w.focus(true);
  assert.equal(asks, 2);
});

test("turned on, the material goes in before the page turns clear", async () => {
  const w = await boot({ focused: true });
  w.settings.updateSettings({ translucency: "subtle" });
  await w.settle();
  assert.deepEqual(w.asked, [true]);
  assert.equal(w.look, undefined, "clear before the window has the material");
  await w.drain();
  assert.equal(w.native, true);
  assert.equal(w.look, "subtle");
});

test("Subtle to Strong, and a theme switch, only retint the page", async () => {
  const w = await boot({ focused: true });
  w.settings.updateSettings({ translucency: "subtle" });
  await w.drain();
  w.settings.updateSettings({ translucency: "strong" });
  w.settings.updateSettings({ appearance: "dark", darkTheme: "dim" });
  w.settings.updateSettings({ appearance: "light" });
  await w.frame(3);
  assert.deepEqual(w.asked, [true]);
  assert.equal(w.look, "strong");
});

test("turned off, the page goes solid at once and the material leaves two frames later", async () => {
  const w = await boot({ focused: true });
  w.settings.updateSettings({ translucency: "strong" });
  await w.drain();
  w.settings.updateSettings({ translucency: "off" });
  assert.equal(w.look, undefined);
  await w.frame(1);
  assert.deepEqual(w.asked, [true], "still on after one frame");
  await w.frame(1);
  await w.drain();
  assert.deepEqual(w.asked, [true, false]);
  assert.equal(w.native, false);
});

test("on, off and on again within a frame ends with the material on and the page clear", async () => {
  const w = await boot({ focused: true });
  w.settings.updateSettings({ translucency: "subtle" });
  w.settings.updateSettings({ translucency: "off" });
  w.settings.updateSettings({ translucency: "strong" });
  await w.frame(3);
  await w.drain();
  assert.equal(w.asked.at(-1), true);
  assert.equal(w.native, true);
  assert.equal(w.look, "strong");
});

test("off, on and off again within a frame ends solid with the material gone", async () => {
  const w = await boot({ focused: true, stored: { translucency: "subtle" } });
  await w.drain();
  w.settings.updateSettings({ translucency: "off" });
  w.settings.updateSettings({ translucency: "subtle" });
  w.settings.updateSettings({ translucency: "off" });
  await w.drain();
  assert.equal(w.look, undefined, "a late answer to the 'on' must not clear the page");
  await w.frame(3);
  await w.drain();
  assert.equal(w.native, false);
  assert.equal(w.look, undefined);
});

test("with the main thread held up, a stale answer never shows a clear page over a window without the material", async () => {
  const w = await boot({ focused: true });
  // Subtle, Off, Subtle while the main thread runs something else: the first answer comes back
  // after the 'off' and the second 'on' are already queued behind it.
  w.settings.updateSettings({ translucency: "subtle" });
  // Past the Reduce transparency question, so the 'on' is asked of the window.
  await w.settle();
  w.settings.updateSettings({ translucency: "off" });
  await w.frame(2);
  w.settings.updateSettings({ translucency: "subtle" });
  await w.settle();
  assert.deepEqual(
    w.pending.map((p) => p.on),
    [true, false, true],
  );
  while (w.pending.length) {
    w.pending.shift()!.resolve();
    await w.settle();
    if (w.look) assert.equal(w.native, true, `clear page with the material ${w.native ? "on" : "off"}, ${w.pending.length} call(s) still queued`);
  }
  assert.equal(w.look, "subtle");
});

test("clicking through the levels while the main thread is busy settles on the last one", async () => {
  const w = await boot({ focused: true });
  for (const level of ["subtle", "off", "strong", "off", "subtle", "strong", "off", "subtle"] as const) {
    w.settings.updateSettings({ translucency: level });
    await w.frame(1);
  }
  await w.frame(3);
  await w.drain();
  assert.equal(w.native, true);
  assert.equal(w.look, "subtle");
  for (const level of ["off", "strong", "off"] as const) {
    w.settings.updateSettings({ translucency: level });
    await w.frame(1);
  }
  await w.frame(3);
  await w.drain();
  assert.equal(w.native, false);
  assert.equal(w.look, undefined);
});

test("in the background the page is solid; the material stays, so nothing is asked of the window", async () => {
  const w = await boot({ focused: true, stored: { translucency: "subtle" } });
  await w.drain();
  assert.equal(w.look, "subtle");
  await w.focus(false);
  assert.equal(w.look, undefined);
  await w.focus(true);
  assert.equal(w.look, "subtle");
  assert.deepEqual(w.asked, [true]);
});

test("a focus storm ends the way the window ends, whatever order Reduce transparency answers in", async () => {
  const answers: ((v: boolean) => void)[] = [];
  const w = await boot({ stored: { translucency: "strong" }, reduce: () => new Promise<boolean>((r) => answers.push(r)) });
  await w.drain();
  for (let i = 0; i < 10; i++) {
    await w.focus(true);
    await w.focus(false);
  }
  await w.focus(true);
  // The latest answer first, the stale ones after.
  answers.reverse().forEach((r) => r(false));
  await w.settle();
  assert.equal(w.look, "strong");
  await w.focus(false);
  await w.focus(true);
  await w.focus(false);
  answers.splice(0).forEach((r) => r(false));
  await w.settle();
  assert.equal(w.look, undefined, "blurred last: solid even though answers came after");
  assert.deepEqual(w.asked, [true]);
});

test("Reduce transparency turned on while away: solid on return and the material leaves", async () => {
  let reduce = false;
  const w = await boot({ stored: { translucency: "subtle" }, reduce: async () => reduce });
  await w.drain();
  await w.focus(true);
  assert.equal(w.look, "subtle");
  await w.focus(false);
  reduce = true;
  await w.focus(true);
  assert.equal(w.look, undefined);
  await w.frame(2);
  await w.drain();
  assert.equal(w.native, false);
  await w.focus(false);
  reduce = false;
  await w.focus(true);
  await w.drain();
  assert.equal(w.native, true);
  assert.equal(w.look, "subtle");
});

test("Reduce transparency failing to answer still lets focus show the setting", async () => {
  const w = await boot({ stored: { translucency: "subtle" }, reduce: () => Promise.reject(new Error("gone")) });
  await w.drain();
  await w.focus(true);
  assert.equal(w.look, "subtle");
});

test("launched with it on: the material is asked for once, the page stays solid until the window comes forward", async () => {
  const w = await boot({ stored: { translucency: "strong" } });
  assert.deepEqual(w.asked, [true]);
  await w.drain();
  assert.equal(w.look, undefined, "starts hidden: solid");
  await w.focus(true);
  assert.equal(w.look, "strong");
  assert.deepEqual(w.asked, [true]);
});

test("a set_translucent that fails leaves the page solid, and picking the setting again retries", async () => {
  const w = await boot({ focused: true });
  w.settings.updateSettings({ translucency: "subtle" });
  await w.settle();
  w.pending.shift()!.reject(new Error("no main thread"));
  await w.settle();
  assert.equal(w.look, undefined);
  w.settings.updateSettings({ translucency: "off" });
  await w.frame(2);
  w.settings.updateSettings({ translucency: "subtle" });
  await w.drain();
  assert.equal(w.look, "subtle");
});

test("stored values from older builds or hand edits fall back to Off; valid ones are kept", async () => {
  for (const translucency of [undefined, null, true, 1, "on", "STRONG", "toString", "__proto__", { level: "strong" }]) {
    const w = await boot({ stored: { appearance: "dark", translucency } });
    assert.equal(w.settings.getSettings().translucency, "off", `stored ${JSON.stringify(translucency)}`);
    assert.deepEqual(w.asked, []);
  }
  const w = await boot({ stored: { translucency: "strong" } });
  assert.equal(w.settings.getSettings().translucency, "strong");
});

test("Reset settings turns it off", async () => {
  const w = await boot({ focused: true, stored: { translucency: "strong" } });
  await w.drain();
  w.settings.resetSettings();
  await w.frame(3);
  await w.drain();
  assert.equal(w.settings.getSettings().translucency, "off");
  assert.equal(w.native, false);
  assert.equal(w.look, undefined);
});

test("off macOS a stored level does nothing: no window calls, the page stays solid", async () => {
  for (const platform of ["Linux x86_64", "Win32"]) {
    const w = await boot({ platform, focused: true, stored: { translucency: "strong" } });
    w.settings.updateSettings({ translucency: "subtle" });
    await w.frame(3);
    assert.deepEqual(w.asked, [], platform);
    assert.equal(w.look, undefined, platform);
  }
});
