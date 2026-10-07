import { useSyncExternalStore } from "react";
import { fullDate, isoToUnix, relativeTime } from "@/lib/format";
import { minute } from "@/lib/ui/minute";

/** relativeTime that keeps itself current: re-rendered only when its text changes. */
export function useRelativeTime(unixSeconds: number) {
  return useSyncExternalStore(minute.subscribe, () => relativeTime(unixSeconds));
}

/**
 * "5m ago", kept current, with the full local date and time on hover. `date`: Unix seconds, or
 * GitHub's ISO 8601. `title` replaces the hover's text.
 */
export function RelativeTime({ date, title, className }: { date: number | string; title?: string; className?: string }) {
  const unix = typeof date === "string" ? isoToUnix(date) : date;
  const text = useRelativeTime(unix);
  const valid = Number.isFinite(unix);
  return (
    <time dateTime={valid ? new Date(unix * 1000).toISOString() : undefined} title={title ?? (valid ? fullDate(unix) : undefined)} className={className}>
      {text}
    </time>
  );
}
