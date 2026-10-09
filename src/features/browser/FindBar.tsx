import { CaseSensitive } from "lucide-react";
import { useRef, useState } from "react";
import { BoxButton, FindBox } from "@/components/FindBox";
import { browserApi } from "@/lib/api";
import { NO_OPTIONS } from "@/lib/ui/findQuery";

/** Find in a tab's page (WebKit's own find, which selects each match): the box's state and steps. */
export function useFindInPage(id: string) {
  const [open, setOpen] = useState(false);
  // Each time Find asks for the box: its text is selected again, to type over.
  const [asked, setAsked] = useState(0);
  const [query, setQuery] = useState("");
  const [matchCase, setMatchCase] = useState(false);
  // Whether the last find matched; null before one, or where WebKit can't say.
  const [found, setFound] = useState<boolean | null>(null);
  // The latest find: an answer for a query typed over since says nothing of this one.
  const latest = useRef(0);
  const run = (text: string, backwards: boolean, caseSensitive = matchCase) => {
    const n = ++latest.current;
    if (!text) return setFound(null);
    void browserApi.find(id, text, backwards, caseSensitive).then(
      (f) => n === latest.current && setFound(f),
      () => n === latest.current && setFound(null),
    );
  };
  const show = () =>
    // The keys come back from the page first, or the field would show focus and get none.
    void browserApi
      .focus(id, false)
      .catch(() => {})
      .then(() => {
        setOpen(true);
        setAsked((n) => n + 1);
      });
  return {
    open,
    asked,
    query,
    matchCase,
    found,
    show,
    /** The next match (⌘G), or the one before; with nothing to find yet, the box. */
    step: (backwards: boolean) => (open && query ? run(query, backwards) : show()),
    type: (text: string) => {
      setQuery(text);
      run(text, false);
    },
    toggleCase: () => {
      setMatchCase(!matchCase);
      run(query, false, !matchCase);
    },
    /** Closed, the keys back to the page. */
    close: () => {
      setOpen(false);
      setFound(null);
      void browserApi.focus(id, true).catch(() => {});
    },
  };
}

/**
 * The find box under the address, as the app's other views have it: in a row of its own, as one
 * over the page would have it step aside. WebKit says only whether a query matched, never how often.
 */
export function FindBar({ find }: { find: ReturnType<typeof useFindInPage> }) {
  return (
    <div className="flex shrink-0 justify-end border-b border-border px-2 py-1">
      <FindBox
        query={find.query}
        onQuery={find.type}
        options={{ ...NO_OPTIONS, matchCase: find.matchCase }}
        onOptions={(o) => o.matchCase !== find.matchCase && find.toggleCase()}
        at={find.found === false ? { index: 0, total: 0 } : null}
        uncounted
        onStep={(dir) => find.step(dir < 0)}
        onClose={find.close}
        focus={find.asked}
        toggles={
          <BoxButton label="Match Case" pressed={find.matchCase} onClick={find.toggleCase}>
            <CaseSensitive className="size-4" />
          </BoxButton>
        }
      />
    </div>
  );
}
