import type { IssueLabel } from "@/lib/api";
import { RowFilter } from "@/components/RowFilter";
import { cn } from "@/lib/utils";

/** A label's color is whatever its author typed; only a plain hex reaches the style. */
export function LabelDot({ label, className }: { label: Pick<IssueLabel, "color">; className?: string }) {
  const color = /^[0-9a-f]{6}$/i.test(label.color) ? `#${label.color}` : undefined;
  return <span className={cn("size-1.5 shrink-0 rounded-full bg-subtle", className)} style={color ? { backgroundColor: color } : undefined} />;
}

/** With `onClick`, the issue list filters by the label. */
export function LabelChip({ label, onClick }: { label: IssueLabel; onClick?: () => void }) {
  const look = "inline-flex max-w-40 items-center gap-1 rounded-full border border-border px-1.5 text-[10.5px] leading-4 text-muted-foreground";
  const content = (
    <>
      <LabelDot label={label} />
      <span className="truncate">{label.name}</span>
    </>
  );
  if (!onClick) return <span className={look}>{content}</span>;
  return (
    <RowFilter title={`Filter by label ${label.name}`} onFilter={onClick} className={cn(look, "hover:border-border-strong hover:text-foreground")}>
      {content}
    </RowFilter>
  );
}
