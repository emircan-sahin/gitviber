import { useCallback, useEffect, useRef, useState } from "react";
import { browserApi, type BrowserPick } from "@/lib/api";
import { failed } from "@/lib/app/toast";
import { takePicked, usePicked } from "@/lib/browser/store";

/** A tab's element picker: on until the page says what it picked (browser-picked), then its note. */
export function usePicker(id: string) {
  const [picking, setPicking] = useState(false);
  const [pick, setPick] = useState<BrowserPick | null>(null);
  const picked = usePicked(id);
  useEffect(() => {
    if (picked === undefined) return;
    setPicking(false);
    takePicked(id);
    // The keys come back from the page first, or the note's field would show focus and get none.
    if (picked) void browserApi.focus(id, false).finally(() => setPick(picked)).catch(() => {});
  }, [picked, id]);

  const set = useCallback(
    (on: boolean) => {
      setPicking(on);
      if (on) setPick(null);
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

  return { picking, pick, toggle, closeNote: () => setPick(null) };
}
