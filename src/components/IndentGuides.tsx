/** Indent steps of a tree's rows (Explorer, Changes' tree view): the row's padding and the guide lines. */
export const INDENT = 12;
export const indent = (depth: number) => 8 + depth * INDENT;

export function IndentGuides({ depth }: { depth: number }) {
  return Array.from({ length: depth }, (_, i) => <span key={i} className="absolute inset-y-0 w-px bg-border" style={{ left: 14 + i * INDENT }} />);
}
