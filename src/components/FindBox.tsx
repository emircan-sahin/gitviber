import { ArrowDown, ArrowUp, CaseSensitive, Regex, WholeWord, X } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { IS_MAC } from "@/lib/commands";
import { useFind } from "@/lib/find";
import { findMatches, type FindOptions, NO_OPTIONS, optionKey } from "@/lib/findQuery";
import type { Panel } from "@/lib/panels";
import { cn } from "@/lib/utils";

/**
 * A find box for a view that isn't Monaco (whose own box the code view shows): the query with
 * Monaco's toggles, where the current match is among them all, and ↵ / ⇧↵ to move between them, Esc to close.
 */
export function FindBox({
  query,
  onQuery,
  options,
  onOptions,
  at,
  error,
  onStep,
  onClose,
  focus,
}: {
  query: string;
  onQuery: (q: string) => void;
  options: FindOptions;
  onOptions: (o: FindOptions) => void;
  /** The current match (1-based, 0 when past what's counted) and how many there are; null while not known. */
  at: { index: number; total: number } | null;
  /** Why the query can't run (a regex that doesn't parse). */
  error?: string | null;
  onStep: (dir: 1 | -1) => void;
  onClose: () => void;
  /** Changes whenever Find asks for the box again: its text is selected, to type over. */
  focus: number;
}) {
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, [focus]);
  const none = !!query && (!!error || at?.total === 0);
  return (
    <div data-find-box className="flex items-center gap-0.5 rounded-md border border-border-strong bg-elevated py-0.5 pr-0.5 pl-2 shadow-md shadow-black/30">
      <input
        ref={input}
        value={query}
        onChange={(e) => onQuery(e.target.value)}
        onKeyDown={(e) => {
          if (flipOnKey(e, options, onOptions)) return;
          if (e.key === "Enter") onStep(e.shiftKey ? -1 : 1);
          else if (e.key === "Escape") onClose();
          else return;
          e.preventDefault();
        }}
        placeholder="Find"
        spellCheck={false}
        className="h-6 w-40 min-w-0 bg-transparent text-[12px] outline-none placeholder:text-subtle"
      />
      <FindToggles options={options} onOptions={onOptions} />
      <span title={error ?? undefined} className={cn("min-w-16 shrink-0 text-right text-[11px] whitespace-nowrap tabular-nums", none ? "text-removed" : "text-subtle")}>
        {!query ? "" : error ? "Invalid regex" : !at ? "" : !at.total ? "No results" : at.index ? `${at.index} of ${at.total}` : `${at.total}+`}
      </span>
      <BoxButton label="Previous match (⇧↵)" disabled={!at?.total} onClick={() => onStep(-1)}>
        <ArrowUp className="size-3.5" />
      </BoxButton>
      <BoxButton label="Next match (↵)" disabled={!at?.total} onClick={() => onStep(1)}>
        <ArrowDown className="size-3.5" />
      </BoxButton>
      <BoxButton label="Close (Esc)" onClick={onClose}>
        <X className="size-3.5" />
      </BoxButton>
    </div>
  );
}

function BoxButton({ label, disabled, pressed, onClick, children }: { label: string; disabled?: boolean; pressed?: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      aria-label={label}
      aria-pressed={pressed}
      title={label}
      disabled={disabled}
      // Keeps the focus in the box, so ↵ still steps after a click.
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
      className={cn(
        "flex size-6 shrink-0 items-center justify-center rounded-sm hover:bg-hover hover:text-foreground disabled:opacity-40 disabled:hover:bg-transparent",
        pressed ? "bg-primary/20 text-foreground ring-1 ring-primary/60 ring-inset" : "text-subtle",
      )}
    >
      {children}
    </button>
  );
}

const OPTION_KEY = IS_MAC ? "⌥⌘" : "Alt+";

/** Match Case, Whole Word and Regex, as Monaco's find box has them. */
export function FindToggles({ options, onOptions }: { options: FindOptions; onOptions: (o: FindOptions) => void }) {
  const toggle = (k: keyof FindOptions) => onOptions({ ...options, [k]: !options[k] });
  return (
    <>
      <BoxButton label={`Match Case (${OPTION_KEY}C)`} pressed={options.matchCase} onClick={() => toggle("matchCase")}>
        <CaseSensitive className="size-4" />
      </BoxButton>
      <BoxButton label={`Match Whole Word (${OPTION_KEY}W)`} pressed={options.wholeWord} onClick={() => toggle("wholeWord")}>
        <WholeWord className="size-4" />
      </BoxButton>
      <BoxButton label={`Use Regular Expression (${OPTION_KEY}R)`} pressed={options.regex} onClick={() => toggle("regex")}>
        <Regex className="size-3.5" />
      </BoxButton>
    </>
  );
}

/** Flips an option on its key, Monaco's (see optionKey); true when the key was one. */
export function flipOnKey(e: React.KeyboardEvent, options: FindOptions, onOptions: (o: FindOptions) => void) {
  const k = optionKey(e.nativeEvent, IS_MAC);
  if (!k) return false;
  e.preventDefault();
  onOptions({ ...options, [k]: !options[k] });
  return true;
}

/** Whether Find has the box open, and a count that changes each time it asks for it. */
export function useFindBox(panel: Panel) {
  const [open, setOpen] = useState(false);
  const [asked, setAsked] = useState(0);
  useFind(panel, () => {
    setOpen(true);
    setAsked((n) => n + 1);
  });
  return { open, asked, close: () => setOpen(false) };
}

// The Custom Highlight API: WebKit since Safari 17.2. Without it matches aren't painted, only counted and scrolled to.
const highlights = typeof CSS !== "undefined" && "highlights" in CSS ? CSS.highlights : null;

/**
 * Find in a page the app renders itself (Markdown, a pull request, an issue). Put it first in the
 * page's scroller: it stays at the top right as the page scrolls, and searches the rest of it.
 */
export function PageFind() {
  const box = useFindBox("code");
  const [query, setQuery] = useState("");
  const [options, setOptions] = useState(NO_OPTIONS);
  // The current match, for the query it was picked in (a new one starts from the first).
  const [picked, setPicked] = useState({ query: "", index: 0 });
  const anchor = useRef<HTMLDivElement>(null);
  const scroller = () => anchor.current?.parentElement ?? null;

  // The page changed under the matches (comments load, a refresh): look again. Not for the box's own count.
  const [version, setVersion] = useState(0);
  useEffect(() => {
    const root = scroller();
    if (!box.open || !root) return;
    let frame = 0;
    const watch = new MutationObserver((records) => {
      if (records.some((r) => !(r.target instanceof Element ? r.target : r.target.parentElement)?.closest("[data-find-box]"))) frame ||= requestAnimationFrame(() => {
        frame = 0;
        setVersion((v) => v + 1);
      });
    });
    watch.observe(root, { subtree: true, childList: true, characterData: true });
    return () => {
      watch.disconnect();
      cancelAnimationFrame(frame);
    };
  }, [box.open]);
  const found = useMemo(() => {
    const root = scroller();
    return box.open && query && root ? findRanges(root, query, options) : null;
  }, [box.open, query, options, version]);
  const ranges = found instanceof Error ? null : found;
  const index = picked.query === query ? picked.index : 0;
  const current = ranges?.length ? Math.min(index, ranges.length - 1) : -1;

  useLayoutEffect(() => {
    if (!highlights) return;
    if (!ranges?.length) {
      highlights.delete("gv-find");
      highlights.delete("gv-find-current");
      return;
    }
    highlights.set("gv-find", new Highlight(...ranges));
    highlights.set("gv-find-current", new Highlight(ranges[current]));
  }, [ranges, current]);
  useEffect(() => () => void (highlights?.delete("gv-find"), highlights?.delete("gv-find-current")), []);

  // Scrolls to the match once the user has moved to it (typed, stepped, opened the box), not whenever the page changes under it.
  const reveal = useRef(false);
  useEffect(() => {
    reveal.current = true;
  }, [box.asked]);
  useEffect(() => {
    const root = scroller();
    if (!reveal.current || !ranges || !root) return;
    reveal.current = false;
    if (current >= 0) scrollToRange(root, ranges[current]);
  }, [ranges, current]);

  const onQuery = (q: string) => {
    setQuery(q);
    reveal.current = true;
  };
  const onOptions = (o: FindOptions) => {
    setOptions(o);
    setPicked({ query: "", index: 0 });
    reveal.current = true;
  };
  const step = (dir: 1 | -1) => {
    if (!ranges?.length) return;
    setPicked({ query, index: (current + dir + ranges.length) % ranges.length });
    reveal.current = true;
  };
  const close = () => {
    box.close();
    scroller()?.focus();
  };

  return (
    <div ref={anchor} className="sticky top-0 z-10 h-0">
      {box.open && (
        <div className="absolute top-2 right-4">
          <FindBox
            query={query}
            onQuery={onQuery}
            options={options}
            onOptions={onOptions}
            at={ranges && { index: current + 1, total: ranges.length }}
            error={found instanceof Error ? found.message : null}
            onStep={step}
            onClose={close}
            focus={box.asked}
          />
        </div>
      )}
    </div>
  );
}

/** Every place `root`'s text matches, across elements too (a word half in a link), but not in the find box. */
export function findRanges(root: HTMLElement, query: string, options: FindOptions): Range[] | Error {
  const nodes: Text[] = [];
  const starts: number[] = [];
  let text = "";
  const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode: (n) => (n.parentElement?.closest("[data-find-box]") ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT),
  });
  for (let n = walk.nextNode(); n; n = walk.nextNode()) {
    nodes.push(n as Text);
    starts.push(text.length);
    text += n.nodeValue;
  }
  const matches = findMatches(text, query, options);
  if (matches instanceof Error) return matches;
  const out: Range[] = [];
  // The node holding character `at` (the last one starting at or before it); `i` only moves forward.
  let i = 0;
  const locate = (at: number): [Text, number] => {
    while (i + 1 < nodes.length && starts[i + 1] <= at) i++;
    return [nodes[i], at - starts[i]];
  };
  for (const [at, end] of matches) {
    const range = document.createRange();
    range.setStart(...locate(at));
    // The end lands in the node holding its last character, not at the start of the next one.
    while (i + 1 < nodes.length && starts[i + 1] < end) i++;
    range.setEnd(nodes[i], end - starts[i]);
    out.push(range);
  }
  return out;
}

/** Brings `range` into view in `root`, a third of the way down, only when it's off screen. */
function scrollToRange(root: HTMLElement, range: Range) {
  // A wide code block scrolls on its own.
  const pre = range.startContainer.parentElement?.closest("pre");
  if (pre) {
    const [at, box] = [range.getBoundingClientRect(), pre.getBoundingClientRect()];
    if (at.left < box.left || at.right > box.right) pre.scrollLeft += at.left - box.left - box.width / 3;
  }
  const at = range.getBoundingClientRect();
  const view = root.getBoundingClientRect();
  if (at.top < view.top || at.bottom > view.bottom) root.scrollTop += at.top - view.top - view.height / 3;
}
