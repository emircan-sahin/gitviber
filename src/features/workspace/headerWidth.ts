// PanelHeader's fit width. Pure, so it runs under `node --test`.

/**
 * The width a PanelHeader needs to show all of itself: its first child, the row of tabs,
 * unscrolled; its last, the buttons pinned to the right; no room to spare.
 */
export function headerWidth(header: Element): number {
  const box = header.getBoundingClientRect();
  const tabs = header.firstElementChild;
  const buttons = header.lastElementChild;
  if (!tabs || !buttons) return box.width;
  const b = buttons.getBoundingClientRect();
  // The padding either side; on the right only while the buttons fit (in a collapsed panel they don't).
  return Math.ceil(tabs.getBoundingClientRect().left - box.left + tabs.scrollWidth + b.width + Math.max(0, box.right - b.right));
}
