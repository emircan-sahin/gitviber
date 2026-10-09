import { useCallback, useEffect, useRef, useState } from "react";
import { browserApi, type BrowserPick } from "@/lib/api";
import { failed } from "@/lib/app/toast";
import { takePicked, usePicked } from "@/lib/browser/store";

/**
 * A tab's element picker: on until the page says what it picked (browser-picked), then what's
 * asked about them. ⇧-clicks gather a few on the way; a plain click picks the last.
 */
export function usePicker(id: string) {
  const [picking, setPicking] = useState(false);
  const [picks, setPicks] = useState<BrowserPick[] | null>(null);
  const gathered = useRef<BrowserPick[]>([]);
  const picked = usePicked(id);
  useEffect(() => {
    if (!picked) return;
    takePicked(id);
    let done: BrowserPick[] | null = null;
    for (const p of picked) {
      if (p?.more) {
        gathered.current.push(p);
        continue;
      }
      setPicking(false);
      done = p ? [...gathered.current, p] : null;
      gathered.current = [];
    }
    // The keys come back from the page first, or the question's field would show focus and get none.
    if (done) void browserApi.focus(id, false).finally(() => setPicks(done)).catch(() => {});
  }, [picked, id]);

  const set = useCallback(
    (on: boolean) => {
      setPicking(on);
      gathered.current = [];
      if (on) setPicks(null);
      // Into the page: Esc there ends the picker.
      void browserApi
        .pick(id, on)
        .then(() => (on ? browserApi.focus(id, true) : undefined))
        .catch(failed("Could not pick an element"));
    },
    [id],
  );
  const toggle = () => set(!picking);

  // Off as the tab hides, or it would still swallow the page's clicks and ⇧⌘C would turn it on again.
  const on = useRef(picking);
  on.current = picking;
  useEffect(() => () => void (on.current && browserApi.pick(id, false).catch(() => {})), [id]);
  // Esc in the app too: a click outside the page took the keys from it.
  useEffect(() => {
    if (!picking) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && set(false);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [picking, set]);

  return { picking, picks, toggle, closeAsk: () => setPicks(null) };
}
