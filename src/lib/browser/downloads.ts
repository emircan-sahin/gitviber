import { formatBytes } from "../format.ts";

// What a user's tab downloads, as the backend tells it (browser/user_downloads.rs): one entry per
// download, kept in the tab until it's dismissed.

export interface BrowserDownload {
  tab: string;
  key: number;
  name: string;
  state: "going" | "done" | "failed" | "cancelled";
  received: number;
  /** -1 until the server says. */
  total: number;
  path: string | null;
  why: string | null;
}

/** The most entries a tab keeps (rows under its address); the oldest ended one makes room. */
const MOST = 4;

/**
 * `list` with this report of a download: a new entry, or the entry of its key changed. Not one
 * `dismissed` already, nor a new one when the list is full of downloads going.
 */
export function reported(list: readonly BrowserDownload[], now: BrowserDownload, dismissed: ReadonlySet<number> = new Set()): BrowserDownload[] {
  if (dismissed.has(now.key)) return [...list];
  if (list.some((d) => d.key === now.key)) return list.map((d) => (d.key === now.key ? now : d));
  const next = [...list];
  while (next.length >= MOST) {
    const ended = next.findIndex((d) => d.state !== "going");
    if (ended < 0) return next;
    next.splice(ended, 1);
  }
  return [...next, now];
}

/** The folder a saved file went to, by name: "Downloads". */
export function folderName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts.length > 1 ? parts[parts.length - 2] : "Downloads";
}

/** How far it is, or where it went, or why it didn't. */
export function downloadText(d: BrowserDownload): string {
  switch (d.state) {
    case "going":
      return d.total > 0 ? `${Math.min(100, Math.floor((d.received / d.total) * 100))}% of ${formatBytes(d.total)}` : d.received > 0 ? formatBytes(d.received) : "Starting";
    case "done":
      return `Saved to ${d.path ? folderName(d.path) : "Downloads"}`;
    case "cancelled":
      return "Cancelled";
    case "failed":
      return d.why ? `Failed: ${d.why}` : "Failed";
  }
}
