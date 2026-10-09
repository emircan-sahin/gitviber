import { CaseSensitive, ChevronDown, ChevronUp, X } from "lucide-react";
import { useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tip } from "@/components/ui/tooltip";
import { browserApi } from "@/lib/api";

/** Find in a tab's page (WebKit's own find, which selects each match): the bar's state and steps. */
export function useFindInPage(id: string) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  // Whether the last find matched; null before one, or where WebKit can't say.
  const [found, setFound] = useState<boolean | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const run = (text: string, backwards: boolean, matchCase = caseSensitive) => {
    if (!text) return setFound(null);
    void browserApi.find(id, text, backwards, matchCase).then(setFound, () => setFound(null));
  };
  const show = () =>
    // The keys come back from the page first, or the field would show focus and get none.
    void browserApi
      .focus(id, false)
      .catch(() => {})
      .then(() => {
        setOpen(true);
        input.current?.focus();
        input.current?.select();
      });
  return {
    open,
    query,
    caseSensitive,
    found,
    input,
    show,
    /** The next match (⌘G), or the one before; with nothing to find yet, the bar. */
    step: (backwards: boolean) => (open && query ? run(query, backwards) : show()),
    type: (text: string) => {
      setQuery(text);
      run(text, false);
    },
    toggleCase: () => {
      setCaseSensitive(!caseSensitive);
      run(query, false, !caseSensitive);
    },
    /** Closed, the keys back to the page. */
    close: () => {
      setOpen(false);
      setFound(null);
      void browserApi.focus(id, true).catch(() => {});
    },
  };
}

/** The find bar under the address, as the pick note is: one over the page would have it step aside. */
export function FindBar({ find }: { find: ReturnType<typeof useFindInPage> }) {
  return (
    <div className="flex h-9 shrink-0 items-center gap-1 border-b border-border px-2 text-[12px]">
      <Input
        ref={find.input}
        autoFocus
        aria-label="Find in page"
        placeholder="Find in page"
        value={find.query}
        onChange={(e) => find.type(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.nativeEvent.isComposing && e.keyCode !== 229) find.step(e.shiftKey);
          else if (e.key === "Escape") find.close();
          else return;
          e.preventDefault();
        }}
        className="h-6 max-w-72 min-w-0 flex-1"
      />
      {find.found === false && <span className="shrink-0 px-1 text-muted-foreground">No matches</span>}
      <Tip label="Match Case">
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          aria-label="Match case"
          aria-pressed={find.caseSensitive}
          onClick={find.toggleCase}
          className={find.caseSensitive ? "bg-active text-foreground" : undefined}
        >
          <CaseSensitive />
        </Button>
      </Tip>
      <Tip label="Previous Match">
        <Button type="button" variant="ghost" size="icon-sm" aria-label="Previous match" disabled={!find.query} onClick={() => find.step(true)}>
          <ChevronUp />
        </Button>
      </Tip>
      <Tip label="Next Match">
        <Button type="button" variant="ghost" size="icon-sm" aria-label="Next match" disabled={!find.query} onClick={() => find.step(false)}>
          <ChevronDown />
        </Button>
      </Tip>
      <div className="flex-1" />
      <Tip label="Close (Esc)">
        <Button type="button" variant="ghost" size="icon-sm" aria-label="Close find" onClick={find.close}>
          <X />
        </Button>
      </Tip>
    </div>
  );
}
