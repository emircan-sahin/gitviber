import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { register } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

// The real usePanelSizes against the real react-resizable-panels layout code: the library's own
// sources, from its source map, driven the way its Group and ResizeObserver drive them.
const SRC = new URL("../../", import.meta.url).href;
register(
  "data:text/javascript," +
    encodeURIComponent(`
export async function resolve(spec, ctx, next) {
  if (spec.startsWith("@/")) spec = ${JSON.stringify(SRC)} + spec.slice(2);
  try {
    return await next(spec, ctx);
  } catch (e) {
    if (!spec.startsWith(".") && !spec.startsWith("file:")) throw e;
    return await next(spec + ".ts", ctx);
  }
}`),
);

const require = (p: string) => fileURLToPath(new URL(p, import.meta.url));
const lib = mkdtempSync(join(tmpdir(), "rrp-"));
const map = JSON.parse(readFileSync(require("../../../node_modules/react-resizable-panels/dist/react-resizable-panels.js.map"), "utf8"));
map.sources.forEach((s: string, i: number) => {
  const p = join(lib, s.replace(/^(\.\.\/)+/, ""));
  mkdirSync(dirname(p), { recursive: true });
  writeFileSync(p, map.sourcesContent[i]);
});
const L = (p: string) => import(pathToFileURL(join(lib, "lib/global", p)).href);

const g = globalThis as Record<string, any>;
const store = new Map<string, string>();
g.localStorage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => store.set(k, v) };
class El {
  w = 0;
  get offsetWidth() {
    return this.w;
  }
  get offsetHeight() {
    return this.w;
  }
  hasAttribute(a: string) {
    return a === "data-panel";
  }
}
g.HTMLElement = El;

const { createElement } = await import("react");
const { renderToString } = await import("react-dom/server");
// Its "@/" imports are beyond this tsconfig: the part of its type used here.
type Ref = { current: unknown };
interface Sides {
  group: { elementRef: Ref; onLayoutChanged(layout: Record<string, number>, meta: { isUserInteraction: boolean }): void };
  panel(id: string): { panelRef: Ref; defaultSize: number | string; groupResizeBehavior: string };
  isCollapsed(id: string): boolean;
  expand(id: string): void;
  collapse(id: string): void;
}
const { usePanelSizes } = (await import(`${SRC}lib/ui/usePanelSizes.ts`)) as { usePanelSizes: (key: string, orientation: string, bases: Record<string, number | string>) => Sides };
const { openSize } = await import("./panelSizes.ts");
const groups = await L("mutable-state/groups.ts");
const { calculatePanelConstraints } = await L("dom/calculatePanelConstraints.ts");
const { calculateDefaultLayout } = await L("utils/calculateDefaultLayout.ts");
const { preserveFixedPanelSizes } = await L("utils/preserveFixedPanelSizes.ts");
const { validatePanelGroupLayout } = await L("utils/validatePanelGroupLayout.ts");
const { layoutsEqual } = await L("utils/layoutsEqual.ts");
const { adjustLayoutByDelta } = await L("utils/adjustLayoutByDelta.ts");
const { getImperativePanelMethods } = await L("utils/getImperativePanelMethods.ts");

// As Workspace has them.
const SIDES = { list: 320, files: 260 };
const IDS = ["list", "viewer", "files"] as const;
const MIN = { list: 240, viewer: 360, files: 200 };
const MAX = { list: 0.45, files: 0.4 };
const CONSTRAINTS: Record<string, object> = { list: { minSize: 240, maxSize: "45", collapsible: true }, viewer: { minSize: 360 }, files: { minSize: 200, maxSize: "40", collapsible: true } };
type Px = Record<(typeof IDS)[number], number>;
type Saved = Record<string, { size?: number; collapsed?: boolean }>;

class Unsettled extends Error {}
let groupId = 0;

/** The sidebars' group at `width` pixels (handles left out), with `saved` as an earlier run left it. */
function workspace(width: number, saved?: unknown) {
  store.clear();
  if (saved) store.set("sides", JSON.stringify(saved));
  let api!: Sides;
  renderToString(createElement(() => ((api = usePanelSizes("sides", "horizontal", SIDES)), null)));
  const id = `g${++groupId}`;
  const els = IDS.map(() => new El());
  const panels = IDS.map((pid, i) => {
    const own = pid === "viewer" ? {} : api.panel(pid);
    return { element: els[i], id: pid, idIsStable: true, mutableValues: {}, panelConstraints: { ...CONSTRAINTS[pid], ...own } };
  });
  const group = { disabled: false, element: { children: els }, id, mutableState: { defaultLayout: undefined, disableCursor: false, expandedPanelSizes: {}, layouts: {} }, orientation: "horizontal", panels, separators: [] };
  api.group.elementRef.current = group.element;
  for (const pid of ["list", "files"]) api.panel(pid).panelRef.current = getImperativePanelMethods({ groupId: id, panelId: pid });

  let W = width;
  // Each panel's offsetWidth: its share of the group, rounded, the rounding left to the last.
  const flow = (layout: Record<string, number>) => {
    let used = 0;
    IDS.forEach((pid, i) => {
      els[i].w = i === IDS.length - 1 ? W - used : Math.round((layout[pid] / 100) * W);
      used += els[i].w;
    });
  };
  let tasks: (() => void)[] = [];
  let calls = 0;
  // Runs `f` and the microtasks the hook queues, as the browser would before the next frame.
  const act = (f: () => void) => {
    const own = g.queueMicrotask;
    g.queueMicrotask = (t: () => void) => tasks.push(t);
    try {
      f();
      for (let i = 0; tasks.length; i++) {
        if (i > 200) throw new Unsettled(`never settles: ${calls} layout changes`);
        tasks.shift()?.();
      }
    } finally {
      tasks = [];
      g.queueMicrotask = own;
    }
  };
  let reported: Record<string, number> = {};
  let dragging = false;
  // Group.tsx: onLayoutChanged once a layout differs from the last one reported, not mid-drag.
  const report = (layout: Record<string, number>, user: boolean) => {
    if (dragging || layoutsEqual(reported, layout)) return;
    reported = layout;
    calls++;
    api.group.onLayoutChanged(layout, { isUserInteraction: user });
  };
  flow({ list: 100 / 3, viewer: 100 / 3, files: 100 / 3 });
  const derived = calculatePanelConstraints(group);
  const first = validatePanelGroupLayout({ layout: calculateDefaultLayout(derived), panelConstraints: derived });
  groups.updateMountedGroup(group, { defaultLayoutDeferred: false, derivedPanelConstraints: derived, groupSize: W, layout: first, separatorToPanels: new Map() });
  flow(first);
  const unsubscribe = groups.subscribeToMountedGroup(id, (e: { next: { layout: Record<string, number> }; isUserInteraction: boolean }) => {
    flow(e.next.layout);
    report(e.next.layout, e.isUserInteraction);
  });
  const state = () => groups.getMountedGroupState(id, true);
  try {
    act(() => report(first, false));
  } catch (e) {
    unsubscribe();
    groups.deleteMutableGroup(group);
    throw e;
  }
  return {
    api,
    get width() {
      return W;
    },
    px: () => Object.fromEntries(IDS.map((pid, i) => [pid, els[i].w])) as Px,
    saved: () => JSON.parse(store.get("sides") ?? "{}") as Saved,
    act,
    /** The window resized: the group's ResizeObserver. */
    resizeWindow(w: number) {
      act(() => {
        const s = state();
        W = w;
        flow(s.layout);
        const c = calculatePanelConstraints(group);
        const layout = validatePanelGroupLayout({ layout: preserveFixedPanelSizes({ group, nextGroupSize: w, prevGroupSize: s.groupSize, prevLayout: s.layout }), panelConstraints: c });
        if (!layoutsEqual(s.layout, layout) || s.groupSize !== w) groups.updateMountedGroup(group, { ...s, derivedPanelConstraints: c, groupSize: w, layout });
      });
    },
    /** A drag of the divider after panel `at` (0: list|viewer, 1: viewer|files) by `dx` pixels. */
    drag(at: number, dx: number) {
      act(() => {
        const s = state();
        const layout = adjustLayoutByDelta({ delta: (dx / W) * 100, initialLayout: s.layout, panelConstraints: s.derivedPanelConstraints, pivotIndices: [at, at + 1], prevLayout: s.layout, trigger: "mouse-or-touch" });
        dragging = true;
        if (!layoutsEqual(s.layout, layout)) groups.updateMountedGroup(group, { ...s, layout });
        dragging = false;
        groups.updateMountedGroup(group, state(), { isUserInteraction: true });
      });
    },
    dispose() {
      unsubscribe();
      groups.deleteMutableGroup(group);
    },
  };
}

const LEFT_ALONE = { list: { size: 320 }, files: { size: 260 } };

test("a sidebar collapsed by a drag reopens, and relaunches, at the width it had", () => {
  const w = workspace(1480);
  assert.deepEqual(w.px(), { list: 320, viewer: 900, files: 260 });
  w.drag(0, 60);
  assert.deepEqual(w.saved(), { list: { size: 380 }, files: { size: 260 } });
  w.drag(0, -300);
  assert.equal(w.px().list, 0);
  assert.deepEqual(w.saved().list, { size: 380, collapsed: true });
  w.act(() => w.api.expand("list"));
  assert.equal(w.px().list, 380);
  w.dispose();
  const again = workspace(1480, { list: { size: 380 }, files: { size: 260 } });
  assert.equal(again.px().list, 380);
  again.dispose();
});

test("stored garbage opens the sidebars at their defaults", () => {
  for (const saved of [{ list: { size: "abc" }, files: 7 }, { list: null, files: {} }, [], "x", 42]) {
    const w = workspace(1280, saved);
    assert.deepEqual(w.px(), { list: 320, viewer: 700, files: 260 }, JSON.stringify(saved));
    w.resizeWindow(1300);
    assert.deepEqual(w.px(), { list: 320, viewer: 720, files: 260 }, JSON.stringify(saved));
    w.dispose();
  }
});

// JSON.stringify writes a NaN size as null.
test("a null saved width is ignored", { todo: "usePanelSizes.ts: the restore loop passes null to resize(), which throws in a microtask" }, () => {
  const w = workspace(1280, { files: { size: null } });
  w.resizeWindow(1300);
  assert.equal(w.px().files, 260);
  w.dispose();
});

test("a width saved on a bigger display opens clamped to the max, and stays saved", () => {
  const w = workspace(1280, { list: { size: 2000 } });
  assert.equal(w.px().list, Math.round(1280 * MAX.list));
  assert.deepEqual(w.saved(), { list: { size: 2000 } });
  w.dispose();
});

test("a negative saved width is ignored, not taken as closed", { todo: "usePanelSizes.ts: the restore loop resizes to saved.size without openSize's > 0 check" }, () => {
  const w = workspace(1280, { list: { size: -50 } });
  w.resizeWindow(1300);
  assert.equal(w.px().list, 320);
  w.dispose();
});

test("the layout settles at every window width", { todo: "usePanelSizes.ts: restoring one squeezed sidebar takes from the other, which restores back: an endless microtask loop" }, () => {
  const failures: string[] = [];
  for (const saved of [undefined, LEFT_ALONE, { list: { size: 400 }, files: { size: 300 } }, { list: { size: 2000 }, files: { size: 1800 } }]) {
    for (let width = 560; width <= 1600; width += 20) {
      try {
        workspace(width, saved).dispose();
      } catch (e) {
        if (!(e instanceof Unsettled)) throw e;
        failures.push(`${width}px ${JSON.stringify(saved)}`);
      }
    }
  }
  assert.deepEqual(failures, []);
});

test("a squeeze undone by the window coming back restores the sidebars", { todo: "usePanelSizes.ts: nothing saved yet means nothing restored; a sidebar the squeeze closed stays closed" }, () => {
  const w = workspace(1480);
  w.resizeWindow(900);
  w.resizeWindow(1480);
  assert.deepEqual(w.px(), { list: 320, viewer: 900, files: 260 });
  w.resizeWindow(720);
  w.resizeWindow(1480);
  assert.deepEqual(w.px(), { list: 320, viewer: 900, files: 260 });
  w.dispose();
});

test("only what the user resized is saved", { todo: "usePanelSizes.ts: remembered() saves every sidebar in the layout, squeezed or squeeze-closed ones included" }, () => {
  // The window closed the list; dragging the explorer doesn't make that the user's choice.
  const a = workspace(720, LEFT_ALONE);
  assert.equal(a.px().list, 0);
  a.drag(1, -20);
  assert.deepEqual(a.saved().list, { size: 320 });
  a.dispose();
  // Opening the list in a narrow window squeezes the explorer: that isn't the explorer's new width.
  const b = workspace(1480, { list: { size: 320, collapsed: true }, files: { size: 260 } });
  b.resizeWindow(860);
  b.act(() => b.api.expand("list"));
  assert.deepEqual(b.saved().files, { size: 260 });
  b.resizeWindow(1480);
  assert.equal(b.px().files, 260);
  b.dispose();
});

test("a sidebar the user closed stays closed when the window shrinks", { todo: "the library pours what a %-max explorer gives up into the first panel, the closed list; the restore loop skips closed panels" }, () => {
  const w = workspace(1480, { list: { size: 320, collapsed: true }, files: { size: 600 } });
  assert.equal(w.px().list, 0);
  w.resizeWindow(1000);
  assert.equal(w.px().list, 0);
  w.dispose();
});

// A small seeded PRNG, so a failure names a sequence that replays.
function random(seed: number) {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

class Broken extends Error {
  kind: string;
  constructor(kind: string, detail: string) {
    super(`${kind}: ${detail}`);
    this.kind = kind;
  }
}
const check = (ok: boolean, kind: string, detail: string) => {
  if (!ok) throw new Broken(kind, detail);
};

test("1,000 random sequences of window resizes, drags and toggles keep the invariants", { todo: "fails on the bugs above; each broken invariant names its first seed" }, () => {
  const broken = new Map<string, { count: number; first: string }>();
  for (let seed = 1; seed <= 1000; seed++) {
    const r = random(seed);
    const int = (lo: number, hi: number) => lo + Math.floor(r() * (hi - lo + 1));
    const start: Saved = {};
    for (const id of ["list", "files"]) if (r() < 0.6) start[id] = { size: int(200, 700), ...(r() < 0.2 ? { collapsed: true } : {}) };
    const log: string[] = [`saved ${JSON.stringify(start)}`];
    let w: ReturnType<typeof workspace> | undefined;
    try {
      const W0 = int(900, 2560);
      log.push(`open at ${W0}`);
      w = workspace(W0, start);
      for (let step = 0; step < 20; step++) {
        const before = w.saved();
        const pxBefore = w.px();
        const op = r();
        let user = true;
        if (op < 0.4) {
          const to = int(560, 2560);
          log.push(`window ${to}`);
          w.resizeWindow(to);
          user = false;
        } else if (op < 0.7) {
          const at = int(0, 1);
          const dx = int(-300, 300);
          log.push(`drag ${at} by ${dx}`);
          w.drag(at, dx);
        } else {
          const id = r() < 0.5 ? "list" : "files";
          const open = w.api.isCollapsed(id);
          log.push(`${open ? "expand" : "collapse"} ${id}`);
          w.act(() => (open ? w?.api.expand(id) : w?.api.collapse(id)));
        }
        const px = w.px();
        const saved = w.saved();
        for (const id of ["list", "files"] as const) {
          if (px[id] > 0 && w.width >= MIN.list + MIN.viewer + MIN.files) check(px[id] >= MIN[id] - 1 && px[id] <= Math.ceil(w.width * MAX[id]) + 1, "out of [min, max]", `${id} at ${px[id]}px`);
          // A width is saved only for a sidebar that changed under the user's hand.
          if (JSON.stringify(saved[id]) !== JSON.stringify(before[id])) {
            check(user, "saved on a window resize", id);
            check(px[id] !== pxBefore[id], "saved a size the user didn't touch", `${id} as ${JSON.stringify(saved[id])}, ${px[id]}px`);
          }
        }
        if (w.width >= MIN.list + MIN.viewer + MIN.files) check(px.viewer >= MIN.viewer - 1, "code view under its min", `${px.viewer}px`);
      }
      // Room again: each open sidebar is back at the width it was left at.
      w.resizeWindow(2560);
      const back = w.px();
      const saved = w.saved();
      for (const id of ["list", "files"] as const) {
        if (saved[id]?.collapsed) continue;
        const want = Math.min(Number(openSize(saved[id], SIDES[id])), Math.round(2560 * MAX[id]));
        check(Math.abs(back[id] - want) <= 1, "not restored with room", `${id} at ${back[id]}px, left at ${want}px`);
      }
      // And no drift: a squeeze and back, ten times over, lands on the same widths.
      for (let i = 0; i < 10; i++) {
        w.resizeWindow(int(560, 1200));
        w.resizeWindow(2560);
      }
      check(JSON.stringify(w.px()) === JSON.stringify(back), "drifts over squeeze cycles", `${JSON.stringify(back)} became ${JSON.stringify(w.px())}`);
    } catch (e) {
      const kind = e instanceof Broken ? e.kind : e instanceof Unsettled ? "never settles" : "throws";
      const seen = broken.get(kind) ?? { count: 0, first: `seed ${seed}: ${(e as Error).message}\n    ${log.join("\n    ")}` };
      seen.count++;
      broken.set(kind, seen);
    } finally {
      w?.dispose();
    }
  }
  assert.equal(broken.size, 0, [...broken].map(([kind, { count, first }]) => `${kind} in ${count} of 1000; first ${first}`).join("\n"));
});
