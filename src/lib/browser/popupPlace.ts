// Where popups of browser tabs go among the workspace's tabs (popupTabs.ts).

/** `tab` right after `beside`, and after the popups `beside` already opened (`kin`), as they came. */
export function insertBeside<T extends { key: string }>(tabs: readonly T[], beside: string, tab: T, kin: ReadonlySet<string>): T[] {
  let at = tabs.findIndex((t) => t.key === beside) + 1;
  if (at === 0) return [...tabs, tab];
  while (tabs[at] && kin.has(tabs[at].key)) at++;
  return [...tabs.slice(0, at), tab, ...tabs.slice(at)];
}

/** Where to go as the popup `active` closes: its opener, when that is still open. */
export function openerOf(active: string | null, tabs: readonly { key: string }[], popups: ReadonlyMap<string, string>): string | null {
  const opener = active ? popups.get(active) : undefined;
  return active && opener && !tabs.some((t) => t.key === active) && tabs.some((t) => t.key === opener) ? opener : null;
}
