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

/** "512 B", "3.4 MB": binary units, a decimal under 10; never "1024 KB", which is 1.0 MB. */
export function formatBytes(n: number) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  // 1023.6 KB would round to "1024 KB": past 1023.5 it's the next unit's.
  while (v >= (i ? 1023.5 : 1024) && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  if (!i) return `${n} B`;
  return `${v.toFixed(v < 9.95 ? 1 : 0)} ${units[i]}`;
}

/** How long something has lasted, as a glance needs it: "<1m", "42m", "3h 5m", "2d". */
export function shortDuration(seconds: number) {
  const m = Math.floor(Math.max(0, seconds) / 60);
  if (m < 1) return "<1m";
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  return h < 24 ? `${h}h ${m % 60}m` : `${Math.floor(h / 24)}d`;
}
