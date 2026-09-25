import { Fragment, useEffect } from "react";
import { bindingsFor, chordKeys, COMMANDS, commandFor, type Overrides } from "@/lib/commands/commands";
import { IS_MAC } from "@/lib/platform";
import { canRun, eventChord, matchesCommand, runsAt, useCommands } from "@/lib/commands/keybindings";
import { focusedPanel, type Panel } from "@/lib/ui/panels";
import { pointerMoved } from "@/lib/ui/pointer";
import { getSettings, useSettings } from "@/lib/settings";
import { createStore } from "@/lib/store";

/**
 * Hold ⌘ by itself for a moment and every shortcut shows over the app, until ⌘ is released.
 * ⌘/ shows the same until the next key or click. It holds the pointer, so nothing underneath
 * hovers through it, and a click on it only closes it, leaving focus where it was.
 */

// Off macOS "cmd" is Ctrl (commands.ts), so that's the key to hold there.
const HOLD_KEY = IS_MAC ? "Meta" : "Control";
const HOLD_MS = 1450;

/** The panel's own command categories, listed first while it has focus. */
const PANEL_FIRST: Record<Panel, string[]> = { code: ["Review", "Diff", "Editor"], git: ["Git", "Review"], explorer: ["Explorer"], terminal: ["Terminal"] };

/** Keys that aren't commands: moving around inside a list or view (useListNav, FileTree, MonacoView), and the system's own. */
const FIXED: { category: string; rows: [string[], string][] }[] = [
  {
    category: "Navigation",
    rows: [
      [["up", "down"], "Move through a list"],
      [["enter"], "Open, keep the tab"],
      [["right", "escape"], "To the code and back"],
      [["left", "right"], "Collapse / expand in a tree"],
      [["shift+f10"], "Row menu"],
      [["space", "pageup", "pagedown"], "Scroll the code view"],
      [["tab"], "Next control"],
    ],
  },
  {
    category: "System",
    rows: [
      [["cmd+c"], "Copy"],
      [["cmd+x"], "Cut"],
      [["cmd+v"], "Paste"],
      [["cmd+a"], "Select all"],
      ...(IS_MAC
        ? ([
            [["cmd+h"], "Hide GitViber"],
            [["cmd+m"], "Minimize"],
            [["cmd+`"], "Next window"],
            [["cmd+q"], "Quit"],
          ] as [string[], string][])
        : []),
    ],
  },
];

/** Between two chords: a range (⌘1 … ⌘8) rather than alternatives. */
const THROUGH = "…";

const shown = createStore<"held" | "pinned" | null>(null);
const set = shown.set;

/** `keys` are chords, each an alternative, or THROUGH between two. */
type Row = { id: string; title: string; keys: readonly string[]; where?: string; off?: boolean };

/**
 * Every bound command by category. A global one is dimmed when none of its keys would run it from
 * where focus is (no handler, a dialog in the way, a text field or the terminal eating the key, an
 * earlier command taking the chord). A local one works in its own place, which it names instead.
 */
function groups(overrides: Overrides) {
  const at = { target: document.activeElement };
  const byCategory = new Map<string, Row[]>();
  const add = (category: string, row: Row) => byCategory.set(category, [...(byCategory.get(category) ?? []), row]);
  for (const c of COMMANDS) {
    const keys = bindingsFor(c.id, overrides);
    if (!keys.length) continue;
    // ⌘1–⌘8 read as one row.
    if (/^tab\.goto[2-8]$/.test(c.id)) continue;
    if (c.id === "tab.goto1") {
      const last = bindingsFor("tab.goto8", overrides)[0];
      add(c.category, { id: c.id, title: "Go to Tab 1–8", keys: last ? [keys[0], THROUGH, last] : [keys[0]], off: !canRun(c.id) });
      continue;
    }
    if ("local" in c) {
      add(c.category, { id: c.id, title: c.title, keys, where: c.local });
      continue;
    }
    const off = !canRun(c.id) || !keys.some((k) => commandFor(k, overrides) === c && runsAt(at, k, c));
    add(c.category, { id: c.id, title: c.title, keys, off });
  }
  const panel = focusedPanel();
  const order = [...new Set([...(panel ? PANEL_FIRST[panel] : []), ...byCategory.keys()])];
  return [
    ...order.filter((c) => byCategory.has(c)).map((category) => ({ category, rows: byCategory.get(category)! })),
    ...FIXED.map((g) => ({ category: g.category, rows: g.rows.map(([keys, title]): Row => ({ id: title, title, keys })) })),
  ];
}

export function ShortcutOverlay() {
  const state = shown.use();
  const { keybindings } = useSettings();

  useCommands({ "workbench.shortcutOverlay": () => set(shown.get() === "pinned" ? null : "pinned") });

  useEffect(() => {
    let timer = 0;
    const cancel = () => {
      clearTimeout(timer);
      timer = 0;
    };
    const hide = () => {
      cancel();
      set(null);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === HOLD_KEY) {
        // Alone: with ⇧ or ⌥ it's the start of a chord. Macs don't repeat modifiers; other systems do.
        const alone = !e.altKey && !e.shiftKey && !(IS_MAC ? e.ctrlKey : e.metaKey);
        // The other ⌘ going down restarts it: one timer, which the first keyup cancels.
        cancel();
        // Held over a link (the code view's underline, the terminal's pointer), it's a ⌘-click about to happen.
        if (alone && !e.repeat && !shown.get() && getSettings().shortcutOverlay) timer = window.setTimeout(() => !document.querySelector(".goto-definition-link, .detected-link-active, .xterm-cursor-pointer") && set("held"), HOLD_MS);
        return;
      }
      cancel();
      // Its own key toggles it (the command above), wherever it was shown from.
      if (!shown.get() || matchesCommand("workbench.shortcutOverlay", e)) return;
      // Shown by key, it waits for ⌘ to go down again and the next real key, which still does its job.
      if (shown.get() === "pinned" && !eventChord(e)) return;
      set(null);
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
      }
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key !== HOLD_KEY) return;
      cancel();
      if (shown.get() === "held") set(null);
    };
    // ⌘ held while the pointer travels is a ⌘-click on its way, which the overlay would swallow.
    // Capture runs before pointer.ts's own listener, so pointerMoved still sees the last position.
    const onMouseMove = (e: MouseEvent) => timer && pointerMoved(e) && cancel();
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("mousemove", onMouseMove, true);
    window.addEventListener("pointerdown", cancel, true);
    // ⌘Tab leaves without a keyup.
    window.addEventListener("blur", hide);
    return () => {
      hide();
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("mousemove", onMouseMove, true);
      window.removeEventListener("pointerdown", cancel, true);
      window.removeEventListener("blur", hide);
    };
  }, []);

  if (!state) return null;
  const list = groups(keybindings);
  // Above the scrollbar layer (index.css .sb-layer, z-index 70). No backdrop-filter: over the terminal's WebGL canvas it isn't safe in WebKit.
  return (
    <div
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => set(null)}
      onContextMenu={(e) => {
        e.preventDefault();
        set(null);
      }}
      className="fixed inset-0 z-[80] flex items-center justify-center bg-black/50 p-6 select-none motion-safe:animate-in motion-safe:fade-in-0 motion-safe:duration-150"
    >
      <div className="flex max-h-full w-full max-w-[1320px] flex-col overflow-hidden rounded-lg border border-border-strong bg-elevated shadow-xl shadow-black/60">
        <div className="flex shrink-0 items-baseline gap-3 border-b border-border px-5 py-3">
          <span className="text-[13px] font-semibold">Keyboard Shortcuts</span>
          <span className="text-[11.5px] text-subtle">Change any of them in Settings → Keyboard Shortcuts</span>
          <span className="ml-auto text-[11.5px] text-subtle">{state === "held" ? `Release ${IS_MAC ? "⌘" : "Ctrl"} to close` : "Any key closes"}</span>
        </div>
        <div className="min-h-0 columns-[17.5rem] gap-8 overflow-hidden px-5 pt-3">
          {list.map((g) => (
            <section key={g.category} className="mb-3.5 break-inside-avoid">
              <div className="mb-1 text-[10.5px] font-semibold tracking-wide text-subtle uppercase">{g.category}</div>
              {g.rows.map((r) => (
                <div key={r.id} className={`flex min-h-[22px] items-center gap-2 py-px text-[11.5px] ${r.off ? "opacity-40" : ""}`}>
                  <span className="min-w-0 flex-1 leading-tight">
                    {r.title}
                    {r.where && <span className="block text-[10.5px] text-subtle">in {r.where}</span>}
                  </span>
                  <span className="flex shrink-0 items-center gap-1.5">
                    {r.keys.map((k, i) =>
                      k === THROUGH ? (
                        <span key={i} className="text-subtle">
                          {k}
                        </span>
                      ) : (
                        <Fragment key={i}>
                          {/* Alternatives, told apart from the keys of one chord. */}
                          {i > 0 && r.keys[i - 1] !== THROUGH && <span className="text-[11px] text-subtle/70">/</span>}
                          <Keycaps chord={k} />
                        </Fragment>
                      ),
                    )}
                  </span>
                </div>
              ))}
            </section>
          ))}
        </div>
      </div>
    </div>
  );
}

/** One keycap per key, in the system font, whose ⌘ ⇧ ⌥ ⌃ glyphs read at this size where a monospace font's run together. */
function Keycaps({ chord }: { chord: string }) {
  return (
    <span className="flex gap-[3px]">
      {chordKeys(chord).map((k, i) => (
        <kbd
          key={i}
          className="flex h-[19px] min-w-[19px] items-center justify-center rounded-[4px] border border-b-2 border-border-strong bg-background px-1 [font-family:system-ui] text-[11.5px] leading-none font-medium text-foreground"
        >
          {k}
        </kbd>
      ))}
    </span>
  );
}
