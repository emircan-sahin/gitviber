import { useSyncExternalStore } from "react";
import { fullDate, isoToUnix, relativeTime } from "@/lib/format";
import { minute } from "@/lib/ui/minute";

/** relativeTime of each, kept current by one subscription: re-rendered only when a text changes. */
export function useRelativeTimes(...unixSeconds: number[]) {
  return useSyncExternalStore(minute.subscribe, () => unixSeconds.map(relativeTime).join("\0")).split("\0");
}

/**
 * "5m ago", kept current, with the full local date and time on hover. `date`: Unix seconds, or
 * GitHub's ISO 8601. `title` replaces the hover's text.
 */
export function RelativeTime({ date, title, className }: { date: number | string; title?: string; className?: string }) {
  const unix = typeof date === "string" ? isoToUnix(date) : date;
  const [text] = useRelativeTimes(unix);
  return <TimeText unix={unix} text={text} title={title} className={className} />;
}

/** The element itself, for a caller that worked out `text` already (CommitTime). */
export function TimeText({ unix, text, title, className }: { unix: number; text: string; title?: string; className?: string }) {
  const valid = Number.isFinite(unix);
  return (
    <time dateTime={valid ? new Date(unix * 1000).toISOString() : undefined} title={title ?? (valid ? fullDate(unix) : undefined)} className={className}>
      {text}
    </time>
  );
}
