import assert from "node:assert/strict";
import { register } from "node:module";
import { test } from "node:test";

// settings.ts imports "./app/settingsWindow" the bundler way (no extension): let node find the .ts.
// Each boot's ?boot= query goes down to our own modules, so each window gets its own settings.ts
// and settingsWindow.ts (which reads location.search as it loads); @tauri-apps/api is shared, as
// the Rust side is.
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

type SettingsModule = typeof import("./settings.ts");
type Label = "main" | "settings";
interface Sent {
  target: string;
  event: string;
  payload: unknown;
}

const g = globalThis as Record<string, any>;
const store = new Map<string, string>();
// The event plugin as lib.rs has it: a listener's target label picks what reaches it.
const callbacks = new Map<number, (e: unknown) => void>();
const handlers: { event: string; label: string | null; id: number }[] = [];
// With the call's answer: listen() resolves once the listener is registered, as over IPC.
let pendingListens: { h: (typeof handlers)[number]; registered: () => void }[] = [];
let deferListens = false;
let queue: Sent[] = [];
let sends = 0;
let nextId = 1;

function reset() {
  store.clear();
  callbacks.clear();
  handlers.length = 0;
  pendingListens = [];
  deferListens = false;
  queue = [];
  sends = 0;
}

Object.defineProperty(g, "navigator", { value: { platform: "MacIntel", userAgent: "" }, configurable: true, writable: true });
g.localStorage = {
  getItem: (k: string) => store.get(k) ?? null,
  setItem: (k: string, v: string) => store.set(k, v),
};
g.document = { documentElement: { dataset: {}, style: { setProperty() {} } } };
g.window = g;
g.matchMedia = () => ({ matches: true, addEventListener() {} });
g.__TAURI_INTERNALS__ = {
  metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
  transformCallback: (cb: (e: unknown) => void) => {
    callbacks.set(nextId, cb);
    return nextId++;
  },
  invoke: async (cmd: string, args: Record<string, any>) => {
    if (cmd === "plugin:event|listen") {
      const h = { event: args.event, label: args.target?.kind === "Any" ? null : args.target.label, id: args.handler };
      // Registered when the IPC call lands, not as the page asks.
      if (deferListens) return new Promise((registered) => pendingListens.push({ h, registered: () => registered(args.handler) }));
      handlers.push(h);
      return args.handler;
    }
    if (cmd === "plugin:event|emit_to") {
      // Through IPC: a copy, delivered later.
      sends++;
      queue.push({ target: args.target.label, event: args.event, payload: JSON.parse(JSON.stringify(args.payload ?? null)) });
      return null;
    }
    return null;
  },
};

const settle = async () => {
  for (let i = 0; i < 10; i++) await new Promise((r) => setImmediate(r));
};

/** Delivers what was emitted, in `order` (default: as sent), to the listeners there are now. */
async function deliver(order?: (q: Sent[]) => Sent[]) {
  await settle();
  const sent = order ? order(queue) : queue;
  queue = [];
  for (const s of sent) {
    for (const h of handlers) if (h.event === s.event && (h.label === null || h.label === s.target)) callbacks.get(h.id)?.({ event: s.event, id: 0, payload: s.payload });
  }
  await settle();
}

/** A window's page: its own settings.ts, loaded as that window's index.html would. */
async function boot(label: Label): Promise<SettingsModule> {
  g.location = { search: label === "settings" ? "?settings=appearance" : "" };
  const settings: SettingsModule = await import(`./settings.ts?boot=${label}-${Math.random()}`);
  await settle();
  return settings;
}

const stored = () => JSON.parse(store.get("gitviber.settings.v2") ?? "null");
const pick = (s: SettingsModule) => {
  const { codeFontSize, uiScale, wordWrap, blame, suggestEfforts, suggestModels, appearance, signOffRepos } = s.getSettings();
  return { codeFontSize, uiScale, wordWrap, blame, suggestEfforts, suggestModels, appearance, signOffRepos };
};

/** A patch from one of a handful of the settings either window changes. */
function patchAt(i: number) {
  switch (i % 6) {
    case 0:
      return { codeFontSize: 10 + (i % 9) };
    case 1:
      return { wordWrap: i % 4 === 1 };
    case 2:
      return { suggestEfforts: { claude: ["low", "high", "max", ""][i % 4] } };
    case 3:
      return { appearance: (["system", "light", "dark"] as const)[i % 3] };
    case 4:
      return { blame: i % 8 === 4 };
    default:
      return { suggestModels: { codex: `gpt-${i}` } };
  }
}

test("1,000 alternating patches from the two windows converge, with no echo", async () => {
  reset();
  const main = await boot("main");
  const win = await boot("settings");
  let emitted = 0;
  for (let i = 0; i < 1000; i++) {
    const from = i % 2 ? win : main;
    from.updateSettings(patchAt(i));
    emitted++;
    await deliver();
    assert.deepEqual(pick(main), pick(win), `after patch ${i}`);
    assert.deepEqual(pick(main), pick({ getSettings: () => stored() } as SettingsModule), `stored after patch ${i}`);
  }
  // Each patch was sent once, by the window it came from: applying one sends nothing back.
  await deliver();
  assert.equal(sends, emitted);
});

test("a patch from the settings window applies in main and stays stored", async () => {
  reset();
  const main = await boot("main");
  const win = await boot("settings");
  win.updateSettings({ uiScale: 1.1, suggestEfforts: { codex: "xhigh" } });
  await deliver();
  assert.equal(stored().uiScale, 1.1);
  assert.equal(main.getSettings().uiScale, 1.1);
  assert.deepEqual(main.getSettings().suggestEfforts, { codex: "xhigh" });
});

test("Reset all settings in the settings window keeps what the user built up, in both", async () => {
  reset();
  const main = await boot("main");
  const win = await boot("settings");
  main.updateSettings({ signOffRepos: ["/r"], wordWrap: true, suggestEfforts: { claude: "max" } });
  await deliver();
  win.resetSettings();
  await deliver();
  for (const s of [main, win]) {
    assert.deepEqual(s.getSettings().signOffRepos, ["/r"]);
    assert.deepEqual(s.getSettings().suggestEfforts, {});
  }
  assert.deepEqual(pick(main), pick(win));
});

test("saved efforts are checked as they load", async () => {
  reset();
  store.set(
    "gitviber.settings.v2",
    JSON.stringify({ suggestEfforts: { claude: "high", codex: "max", pi: "", opencode: 5, llm: "high", gemini: "low" } }),
  );
  assert.deepEqual((await boot("main")).getSettings().suggestEfforts, { claude: "high", pi: "" });
  for (const bad of [undefined, null, "high", ["high"], 3]) {
    reset();
    store.set("gitviber.settings.v2", JSON.stringify({ suggestEfforts: bad, wordWrap: true }));
    const s = await boot("main");
    assert.deepEqual(s.getSettings().suggestEfforts, {}, JSON.stringify(bad));
    // The rest of an older settings object stays.
    assert.equal(s.getSettings().wordWrap, true);
  }
});

test("guided review settings are checked as they load, and reset to Commit Messages'", async () => {
  reset();
  store.set(
    "gitviber.settings.v2",
    JSON.stringify({ reviewCommand: "codex exec", reviewModels: { claude: "opus", codex: 5, gemini: "x" }, reviewEfforts: { claude: "max", codex: "max", pi: "" }, reviewLanguage: "Turkish" }),
  );
  const s = await boot("main");
  assert.deepEqual(
    (({ reviewCommand, reviewModels, reviewEfforts, reviewLanguage }) => ({ reviewCommand, reviewModels, reviewEfforts, reviewLanguage }))(s.getSettings()),
    { reviewCommand: "codex exec", reviewModels: { claude: "opus" }, reviewEfforts: { claude: "max", pi: "" }, reviewLanguage: "Turkish" },
  );
  s.resetSettings();
  assert.equal(s.getSettings().reviewCommand, null);
  assert.deepEqual(s.getSettings().reviewModels, {});
  assert.equal(s.getSettings().reviewLanguage, "English");
  for (const bad of [{ reviewCommand: 3, reviewModels: "x", reviewEfforts: ["high"], reviewLanguage: null }, {}]) {
    reset();
    store.set("gitviber.settings.v2", JSON.stringify({ ...bad, wordWrap: true }));
    const { reviewCommand, reviewModels, reviewEfforts, reviewLanguage, wordWrap } = (await boot("main")).getSettings();
    assert.deepEqual({ reviewCommand, reviewModels, reviewEfforts, reviewLanguage, wordWrap }, { reviewCommand: null, reviewModels: {}, reviewEfforts: {}, reviewLanguage: "English", wordWrap: true });
  }
});

test("guided review settings changed in the settings window reach main, null included", async () => {
  reset();
  const main = await boot("main");
  const win = await boot("settings");
  win.updateSettings({ reviewCommand: "claude -p", reviewEfforts: { claude: "high" }, reviewLanguage: "日本語" });
  await deliver();
  assert.equal(main.getSettings().reviewCommand, "claude -p");
  assert.deepEqual(main.getSettings().reviewEfforts, { claude: "high" });
  assert.equal(main.getSettings().reviewLanguage, "日本語");
  win.updateSettings({ reviewCommand: null });
  await deliver();
  assert.equal(main.getSettings().reviewCommand, null);
  assert.equal(stored().reviewCommand, null);
});

test("a guided review's model or effort taken back to Commit Messages' in the settings window is gone in main too", async () => {
  reset();
  const main = await boot("main");
  const win = await boot("settings");
  win.updateSettings({ reviewModels: { claude: "opus", codex: "gpt-x" }, reviewEfforts: { claude: "", codex: "high" } });
  await deliver();
  // Review.tsx's without(): the whole object goes, a key left out is Same as again.
  win.updateSettings({ reviewModels: { codex: "gpt-x" }, reviewEfforts: { claude: "" } });
  await deliver();
  assert.deepEqual(main.getSettings().reviewModels, { codex: "gpt-x" });
  // "" (CLI default) stays apart from a key left out (Same as), across a reload too.
  assert.deepEqual(main.getSettings().reviewEfforts, { claude: "" });
  assert.deepEqual((await boot("main")).getSettings().reviewEfforts, { claude: "" });
  main.resetSettings();
  await deliver();
  assert.deepEqual(win.getSettings().reviewEfforts, {});
});

test("patches from both windows before either hears the other converge", async () => {
  reset();
  const main = await boot("main");
  const win = await boot("settings");
  main.updateSettings({ wordWrap: true });
  win.updateSettings({ blame: true });
  await deliver();
  assert.deepEqual(pick(main), pick(win));
  assert.equal(main.getSettings().wordWrap && main.getSettings().blame, true);
});

test("two quick patches that arrive out of order leave the other window on the newer", async () => {
  reset();
  const main = await boot("main");
  const win = await boot("settings");
  win.updateSettings({ suggestModels: { claude: "claude-sonnet-" } });
  win.updateSettings({ suggestModels: { claude: "claude-sonnet-5" } });
  await deliver((q) => [...q].reverse());
  assert.deepEqual(main.getSettings().suggestModels, { claude: "claude-sonnet-5" });
  assert.deepEqual(stored().suggestModels, { claude: "claude-sonnet-5" });
});

test("a change made while the settings window is still loading reaches it", async () => {
  reset();
  const main = await boot("main");
  deferListens = true;
  const win = await boot("settings");
  main.updateSettings({ wordWrap: true });
  await deliver();
  for (const { h, registered } of pendingListens) {
    handlers.push(h);
    registered();
  }
  await settle();
  deferListens = false;
  // And the next change there doesn't take it back.
  win.updateSettings({ blame: true });
  await deliver();
  assert.equal(win.getSettings().wordWrap, true);
  assert.equal(stored().wordWrap, true);
});

test("a change made in the settings window while main reloads reaches it, and later ones still apply", async () => {
  reset();
  await boot("main");
  const win = await boot("settings");
  deferListens = true;
  const main = await boot("main");
  win.updateSettings({ wordWrap: true });
  await deliver();
  for (const { h, registered } of pendingListens) {
    handlers.push(h);
    registered();
  }
  await settle();
  deferListens = false;
  assert.equal(main.getSettings().wordWrap, true);
  // The reloaded page's sends are newer than anything heard from the one before.
  main.updateSettings({ blame: true });
  await deliver();
  assert.equal(win.getSettings().blame, true);
});
