import { cn } from "@/lib/utils";

/**
 * A part of a list row (an author's name) that narrows the list to it, as GitHub's lists do.
 * Not a tab stop and no aria-label: the row's menu does the same from the keyboard, and the row
 * still reads as its plain text. A press never drags the row; a ⌘, ⇧ or ⌥ click is the row's (it picks).
 */
export function RowFilter({ title, onFilter, className, children }: { title: string; onFilter: () => void; className?: string; children: React.ReactNode }) {
  return (
    <button
      type="button"
      tabIndex={-1}
      title={title}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
        e.stopPropagation();
        onFilter();
      }}
      // The row opens and keeps its item on a double-click.
      onDoubleClick={(e) => e.stopPropagation()}
      className={cn("min-w-0 truncate text-left underline-offset-2 hover:text-foreground hover:underline", className)}
    >
      {children}
    </button>
  );
}
