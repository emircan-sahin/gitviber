// Which rows of a long fixed-height list to render. Pure, so it runs under `node --test`.

/**
 * Rows [first, last) on screen, plus `overscan` either side. `offset`: how far the viewport's top is
 * below the list's top (negative while the list starts further down).
 */
export function visibleRows(offset: number, viewport: number, height: number, count: number, overscan: number): [number, number] {
  const first = Math.max(0, Math.floor(offset / height) - overscan);
  const last = Math.min(count, Math.ceil((offset + viewport) / height) + overscan);
  return [first, Math.max(first, last)];
}

/** The rows to render, in order: the visible range and the `keep` rows, wherever those are. */
export function renderedRows([first, last]: [number, number], keep: number[], count: number): number[] {
  const out = new Set<number>();
  for (let i = first; i < Math.min(last, count); i++) out.add(i);
  for (const i of keep) if (i >= 0 && i < count) out.add(i);
  return [...out].sort((a, b) => a - b);
}
