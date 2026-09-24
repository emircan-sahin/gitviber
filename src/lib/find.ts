import { useEffect, useRef } from "react";
import { focusedPanel, type Panel } from "./panels";

/**
 * Find (⌘F) searches where focus is: each panel's view says what that means there (the code
 * view's find box, a list's filter, the terminal's search). With focus in no panel, the code view.
 */
const finders = new Map<Panel, (() => void)[]>();

export function find() {
  const top = (p: Panel) => finders.get(p)?.at(-1);
  (top(focusedPanel() ?? "code") ?? top("code"))?.();
}

/** Registers what Find does in `panel` while mounted (the last one registered wins); null for nothing. */
export function useFind(panel: Panel, fn: (() => void) | null) {
  const ref = useRef(fn);
  useEffect(() => {
    ref.current = fn;
  });
  const on = !!fn;
  useEffect(() => {
    if (!on) return;
    const run = () => ref.current?.();
    finders.set(panel, [...(finders.get(panel) ?? []), run]);
    return () => void finders.set(panel, (finders.get(panel) ?? []).filter((f) => f !== run));
  }, [panel, on]);
}
