// The update state without Tauri, so node:test runs it (updates.ts drives it).

/** A newer release from latest.json. */
export interface Release {
  version: string;
  /** Markdown, from latest.json's `notes`. */
  notes: string;
  date: string | null;
}

export interface Download {
  received: number;
  /** null when the server sent no length. */
  total: number | null;
}

export interface UpdateState {
  /** The newest release the last check found; null while up to date or not checked yet. */
  release: Release | null;
  /** Its download while it runs, "ready" once it waits for a restart. */
  download: Download | "ready" | null;
  checking: boolean;
  /** When a check last got an answer (ms). */
  checkedAt: number | null;
}

export const INITIAL: UpdateState = { release: null, download: null, checking: false, checkedAt: null };

/** What a check found. A release being downloaded or waiting for a restart stays: that's what installs. */
export function checked(s: UpdateState, found: Release | null, now: number): UpdateState {
  const keep = s.download !== null && s.release !== null;
  return { ...s, release: keep ? s.release : found, checking: false, checkedAt: now };
}

/** The plugin's download events (DownloadEvent in @tauri-apps/plugin-updater). */
export type DownloadEvent = { event: "Started"; data: { contentLength?: number } } | { event: "Progress"; data: { chunkLength: number } } | { event: "Finished" };

/** Progress only: "ready" waits for download() to resolve, as the signature is checked after Finished. */
export function downloaded(s: UpdateState, e: DownloadEvent): UpdateState {
  if (e.event === "Started") return { ...s, download: { received: 0, total: e.data.contentLength || null } };
  if (e.event === "Progress" && typeof s.download === "object" && s.download) return { ...s, download: { ...s.download, received: s.download.received + e.data.chunkLength } };
  return s;
}

/** Whole percent of a download, null when its size is unknown. */
export function percent(d: Download): number | null {
  if (!d.total) return null;
  return Math.min(100, Math.floor((d.received / d.total) * 100));
}

/** Whether an automatic check is due: never checked, or the last answer is `every` ms old. */
export const due = (checkedAt: number | null, now: number, every: number) => checkedAt === null || now - checkedAt >= every;

/**
 * Failures an automatic check keeps to itself: offline, a timeout, GitHub down, or no release
 * published yet (the endpoint 404s). Anything else (a malformed latest.json, a missing platform)
 * is a release problem worth logging.
 */
export const isExpectedFailure = (message: string) => /error sending request|Could not fetch a valid release JSON/i.test(message);
