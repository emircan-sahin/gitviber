/** "feature-a", "feature-a and feature-b", "feature-a, feature-b and 2 more": the branches a rewrite can move along. */
export function moveAlong(branches: string[]): string {
  const shown = branches.slice(0, 3);
  const rest = branches.length - shown.length;
  if (rest > 0) return `${shown.join(", ")} and ${rest} more`;
  return shown.length > 1 ? `${shown.slice(0, -1).join(", ")} and ${shown[shown.length - 1]}` : (shown[0] ?? "");
}
