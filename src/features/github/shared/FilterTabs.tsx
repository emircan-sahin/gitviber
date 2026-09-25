import { cn } from "@/lib/utils";

export type Filter = "open" | "closed" | "all";

/**
 * Open / Closed / All: the sidebar's tabs (Changes, History, …) one size down, so they read as
 * tabs and as a level under them. `counts` badges each, as Changes' count does.
 */
export function FilterTabs<F extends Filter>({
  value,
  onChange,
  counts,
}: {
  value: F;
  onChange: (f: F) => void;
  counts?: Partial<Record<F, string>>;
}) {
  const tabs = ["open", "closed", "all"] as F[];
  // A tablist is one tab stop; ←/→ pick the neighbour, as WAI-ARIA's tabs pattern.
  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const step = { ArrowLeft: -1, ArrowRight: 1 }[e.key];
    if (!step || e.altKey || e.metaKey || e.ctrlKey || e.shiftKey) return;
    const i = (tabs.indexOf(value) + step + tabs.length) % tabs.length;
    onChange(tabs[i]);
    e.currentTarget.querySelectorAll<HTMLElement>('[role="tab"]')[i]?.focus();
    e.preventDefault();
  };
  return (
    <div role="tablist" onKeyDown={onKeyDown} className="flex shrink-0 items-center gap-0.5">
      {tabs.map((f) => {
        const on = value === f;
        return (
          <button
            key={f}
            role="tab"
            aria-selected={on}
            tabIndex={on ? 0 : -1}
            onClick={() => onChange(f)}
            className={cn(
              "flex h-5 items-center gap-1 rounded-sm px-1.5 text-[11.5px] font-medium capitalize",
              on ? "bg-active text-foreground" : "text-subtle hover:text-foreground focus:text-foreground",
            )}
          >
            {f}
            {counts?.[f] !== undefined && (
              // Counts go first when the header runs out of room (IssuesPanel's container).
              <span className={cn("rounded-sm bg-elevated px-1 font-mono text-[10px] leading-3.5 @max-[300px]:hidden", on ? "text-foreground" : "text-muted-foreground")}>
                {counts[f]}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}
