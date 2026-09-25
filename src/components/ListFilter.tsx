import { Search, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useFind } from "@/lib/ui/find";
import { focusPanel, type Panel } from "@/lib/ui/panels";

/**
 * A list's filter: a row like History's search box, which Find (⌘F) opens while focus is in the
 * list's panel. Esc clears it, then closes it; ↓ and ↵ go on to the rows it leaves.
 */
export function useListFilter(panel: Panel, placeholder: string) {
  // null while closed.
  const [query, setQuery] = useState<string | null>(null);
  const [asked, setAsked] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const open = () => {
    setQuery((q) => q ?? "");
    setAsked((n) => n + 1);
  };
  useFind(panel, open);
  useEffect(() => {
    if (!asked) return;
    input.current?.focus();
    input.current?.select();
  }, [asked]);

  const needle = query?.trim().toLowerCase() ?? "";
  const dismiss = () => {
    setQuery(null);
    focusPanel(panel);
  };
  const bar = query !== null && (
    <div className="flex h-8 shrink-0 items-center gap-2 border-b border-border px-2.5">
      <Search className="size-3.5 shrink-0 text-subtle" />
      <input
        ref={input}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") query ? setQuery("") : dismiss();
          else if (e.key === "ArrowDown" || e.key === "Enter") focusPanel(panel);
          else return;
          e.preventDefault();
        }}
        placeholder={placeholder}
        spellCheck={false}
        className="h-full min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-subtle"
      />
      <button aria-label="Close filter" onClick={dismiss} className="flex size-4 shrink-0 items-center justify-center rounded-sm text-subtle hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground">
        <X className="size-3" />
      </button>
    </div>
  );
  return {
    /** The filter's text, trimmed and lowercased; empty while it lets everything through. */
    needle,
    /** Whether any of these texts holds the filter's (any case). */
    matches: (...texts: (string | null | undefined)[]) => !needle || texts.some((t) => t?.toLowerCase().includes(needle)),
    bar,
    /** Shows the row (as Find does) and puts the cursor in it. */
    open,
    /** Clears and hides the row; focus stays where it is. */
    close: () => setQuery(null),
  };
}
