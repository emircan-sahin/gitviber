/** "3h ago", from a Unix time in seconds. */
export function relativeTime(unixSeconds: number) {
  const s = Math.max(0, Date.now() / 1000 - unixSeconds);
  const steps: [number, string][] = [
    [31557600, "y"],
    [2629800, "mo"],
    [604800, "w"],
    [86400, "d"],
    [3600, "h"],
    [60, "m"],
  ];
  for (const [secs, unit] of steps) if (s >= secs) return `${Math.floor(s / secs)}${unit} ago`;
  return "just now";
}

/** A Unix time in seconds, in the user's locale with the time of day: for a tooltip. */
export const fullDate = (unixSeconds: number) => new Date(unixSeconds * 1000).toLocaleString();

/** GitHub's ISO 8601 times as Unix seconds, for relativeTime. */
export const isoToUnix = (iso: string) => Date.parse(iso) / 1000;

/** "1 commit", "3 commits": `word` with an "s" unless there's one. */
export const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
