import { useEffect, useRef } from "react";
import { listenHere } from "../app/settingsWindow";
import { toast } from "../app/toast";
import { type Selection, selectionKey } from "../repo/selection";
import { createStore } from "../store";
import { openerOf } from "./popupPlace";

// Popups of the user's own tabs (window.open, target=_blank, a login window): the backend has made
// the view already, with its opener kept (browser/macos/popups.rs), and the tab joins the workspace
// next to its opener's, brought up as a browser does.

type Tab = { key: string; sel: Selection };
type Popup = { id: string; opener: string; url: string };

const waiting = createStore<readonly Popup[]>([]);
// Each popup tab's key with its opener's, for as long as the popup is open.
const openers = new Map<string, string>();

listenHere<{ id: string | null; opener: string; url: string }>("browser-popup", ({ payload: { id, opener, url } }) => {
  if (id) waiting.set([...waiting.get(), { id, opener, url }]);
}).catch(() => {});

// One toast for a burst of refused popups.
let toldAt = 0;
listenHere("browser-popup-blocked", () => {
  if (Date.now() - toldAt < 5000) return;
  toldAt = Date.now();
  toast("info", "Popup blocked", "This page tried to open several windows at once.");
}).catch(() => {});

const keyOf = (id: string) => selectionKey({ kind: "browser", id, url: "" });

/**
 * Popups of this workspace's browser tabs opened as tabs, `open` putting one beside its opener
 * (after the `kin` already there) and making it the active one; a closed popup returns to its
 * opener.
 */
export function usePopupTabs(tabs: readonly Tab[], activeKey: string | null, open: (sel: Selection, beside: string, kin: ReadonlySet<string>) => void, setActive: (key: string) => void) {
  const queue = waiting.use();
  useEffect(() => {
    const mine = queue.filter((p) => tabs.some((t) => t.key === keyOf(p.opener)));
    if (!mine.length) return;
    waiting.set(queue.filter((p) => !mine.includes(p)));
    for (const { id, opener, url } of mine) {
      const beside = keyOf(opener);
      const kin = new Set([...openers].filter(([, o]) => o === beside).map(([k]) => k));
      openers.set(keyOf(id), beside);
      open({ kind: "browser", id, url }, beside, kin);
    }
  }, [queue, tabs, open]);

  const was = useRef(activeKey);
  useEffect(() => {
    const closed = was.current;
    const back = openerOf(closed, tabs, openers);
    was.current = activeKey;
    if (!back || !closed) return;
    openers.delete(closed);
    setActive(back);
  }, [tabs, activeKey, setActive]);
}
