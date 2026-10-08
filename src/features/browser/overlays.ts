// What this page draws over a browser tab's page. The page is a native view above this one, so
// a menu, a dialog or a toast there would show under it: while one does, the page steps aside.

/** Radix menus, popovers and tooltips; dialogs; and the rest that mark themselves (Toaster, ShortcutOverlay). */
const OVERLAYS = "[data-radix-popper-content-wrapper],[data-modal],[role=dialog],[data-overlay]";
/** A dialog (⌘P, a confirm) owns the keys and the eye wherever it sits. */
const ALWAYS = "[data-modal],[role=dialog]";

/** Whether anything drawn over this page reaches into `r`, a rect in the page's coordinates. */
export function covered(r: DOMRect): boolean {
  for (const el of document.querySelectorAll(OVERLAYS)) {
    if (el.matches(ALWAYS)) return true;
    const o = el.getBoundingClientRect();
    if (o.width > 0 && o.height > 0 && o.left < r.right && o.right > r.left && o.top < r.bottom && o.bottom > r.top) return true;
  }
  return false;
}

const listeners = new Set<() => void>();
let observer: MutationObserver | null = null;

/** Calls `changed` as this page's elements come and go; watched only while someone listens. */
export function watchOverlays(changed: () => void) {
  listeners.add(changed);
  if (!observer) {
    observer = new MutationObserver(() => listeners.forEach((l) => l()));
    observer.observe(document.body, { childList: true, subtree: true });
  }
  return () => {
    listeners.delete(changed);
    if (listeners.size) return;
    observer?.disconnect();
    observer = null;
  };
}
