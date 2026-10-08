// What this page draws over a browser tab's page. The page is a native view above this one, so
// a menu, a dialog or a toast there would show under it: while one does, the page steps aside.

/** Radix menus, popovers and tooltips; dialogs; and the rest that mark themselves (Toaster, ShortcutOverlay). */
const OVERLAYS = "[data-radix-popper-content-wrapper],[data-modal],[role=dialog],[data-overlay]";
/** A dialog (⌘P, a confirm) owns the keys and the eye wherever it sits. */
const ALWAYS = "[data-modal],[role=dialog]";

/** What's drawn over this page and reaches into `r` (the page's rect): a dialog, anything else, or nothing. */
export function coveredBy(r: DOMRect): "dialog" | "overlay" | null {
  let found: "overlay" | null = null;
  for (const el of document.querySelectorAll(OVERLAYS)) {
    if (el.matches(ALWAYS)) return "dialog";
    const o = el.getBoundingClientRect();
    if (o.width > 0 && o.height > 0 && o.left < r.right && o.right > r.left && o.top < r.bottom && o.bottom > r.top) found = "overlay";
  }
  return found;
}

const listeners = new Set<() => void>();
let observer: MutationObserver | null = null;

/** For an overlay that isn't a portal of the body's (a toast, the shortcut overlay): it came or went. */
export function overlaysChanged() {
  listeners.forEach((l) => l());
}

const isOverlay = (n: Node) => n instanceof Element && (n.matches(OVERLAYS) || !!n.querySelector(OVERLAYS));

/** Calls `changed` as overlays come and go; watched only while someone listens. */
export function watchOverlays(changed: () => void) {
  listeners.add(changed);
  // Portals (Radix's menus and dialogs) mount as the body's children: only those are watched.
  observer ??= new MutationObserver((records) => {
    if (records.some((r) => [...r.addedNodes, ...r.removedNodes].some(isOverlay))) overlaysChanged();
  });
  observer.observe(document.body, { childList: true });
  return () => {
    listeners.delete(changed);
    if (listeners.size) return;
    observer?.disconnect();
    observer = null;
  };
}
