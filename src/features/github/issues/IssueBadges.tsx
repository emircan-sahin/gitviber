import type { IssueLabel } from "@/lib/api";
import { cn } from "@/lib/utils";

/** A label's color is whatever its author typed; only a plain hex reaches the style. */
export function LabelDot({ label, className }: { label: Pick<IssueLabel, "color">; className?: string }) {
  const color = /^[0-9a-f]{6}$/i.test(label.color) ? `#${label.color}` : undefined;
  return <span className={cn("size-1.5 shrink-0 rounded-full bg-subtle", className)} style={color ? { backgroundColor: color } : undefined} />;
}

/** With `onClick`, a button: the issue list filters by the label. */
export function LabelChip({ label, onClick }: { label: IssueLabel; onClick?: () => void }) {
  const El = onClick ? "button" : "span";
  return (
    <El
      {...(onClick && {
        title: "Filter by this label",
        // Not a tab stop in every row: the Label filter above does the same from the keyboard.
        tabIndex: -1,
        onClick: (e: React.MouseEvent) => {
          e.stopPropagation();
          onClick();
        },
        // The row opens and pins its issue on a double-click.
        onDoubleClick: (e: React.MouseEvent) => e.stopPropagation(),
      })}
      className={cn(
        "inline-flex max-w-40 items-center gap-1 rounded-full border border-border px-1.5 text-[10.5px] leading-4 text-muted-foreground",
        onClick && "hover:border-border-strong hover:text-foreground focus-visible:text-foreground",
      )}
    >
      <LabelDot label={label} />
      <span className="truncate">{label.name}</span>
    </El>
  );
}
