import { Check, Signal } from "lucide-react";
import { type Category, type GuideSection, sectionBadge } from "@/lib/review/guide";
import { cn } from "@/lib/utils";
import { CATEGORY_UI, CategoryIcon, IMPORTANCE_UI, ImportanceTag } from "./categories";

interface Item {
  n: number;
  section: GuideSection;
  done: boolean;
}

/** One section in the navigator: its category, number, title and importance, checked once reviewed. */
function NavItem({ n, section: s, done, chip, onGo }: Item & { chip: boolean; onGo: (n: number) => void }) {
  return (
    <button
      onClick={() => onGo(n)}
      title={`${CATEGORY_UI[s.category].label}: ${s.title || "Untitled"}. ${IMPORTANCE_UI[s.importance].label} importance${done ? " (reviewed)" : ""}`}
      className={cn(
        "flex min-w-0 items-center gap-1.5 text-left text-[12px] outline-none hover:bg-hover focus-visible:ring-1 focus-visible:ring-ring",
        chip ? "h-6 shrink-0 rounded-full border border-border px-2" : "w-full rounded-sm px-1.5 py-1",
        done && "text-muted-foreground",
      )}
    >
      <CategoryIcon category={s.category} />
      <span className="shrink-0 font-mono text-[11px] text-subtle">{sectionBadge(n)}</span>
      <span className={cn("min-w-0 truncate", chip && "max-w-40")}>{s.title || "Untitled"}</span>
      <span className="sr-only">, {CATEGORY_UI[s.category].label}</span>
      <ImportanceTag importance={s.importance} short />
      {done && (
        <>
          <Check aria-hidden className="size-3.5 shrink-0 text-added" />
          <span className="sr-only">, reviewed</span>
        </>
      )}
    </button>
  );
}

/**
 * The sections shown, to jump to: a rail beside them on a wide tab, a row of chips pinned over
 * them on a narrow one (as high as the view's --stick, which keeps the files' headers below it).
 */
export function GuideNav({ items, onGo }: { items: Item[]; onGo: (n: number) => void }) {
  return (
    <>
      <nav aria-label="Sections" className="sticky top-0 hidden max-h-[calc(100vh-7rem)] self-start overflow-y-auto py-1 @6xl:block">
        {items.map((i) => (
          <NavItem key={i.n} {...i} chip={false} onGo={onGo} />
        ))}
      </nav>
      <nav aria-label="Sections" className="sticky top-0 z-20 -mx-6 flex h-[var(--stick)] items-center gap-1.5 overflow-x-auto border-b border-border bg-background px-6 @6xl:hidden">
        {items.map((i) => (
          <NavItem key={i.n} {...i} chip onGo={onGo} />
        ))}
      </nav>
    </>
  );
}

const chipClass = (on: boolean) =>
  cn(
    "flex h-6 shrink-0 items-center gap-1.5 rounded-md border px-2 text-[12px] outline-none focus-visible:ring-1 focus-visible:ring-ring",
    on ? "border-border-strong bg-active text-foreground" : "border-border text-muted-foreground hover:bg-hover hover:text-foreground",
  );

/**
 * "All · Security 2 · Tests 3": a button a category, the one picked showing only its sections; with
 * `high` (the high importance sections' count, when some but not all are), a toggle for only those too.
 */
export function CategoryFilter({
  counts,
  total,
  value,
  onChange,
  high,
}: {
  counts: Map<Category, number>;
  total: number;
  value: Category | null;
  onChange: (c: Category | null) => void;
  high: { count: number; on: boolean; onChange: (on: boolean) => void } | null;
}) {
  const button = (c: Category | null, label: string, n: number) => (
    <button key={c ?? "all"} aria-pressed={value === c} onClick={() => onChange(c)} className={chipClass(value === c)}>
      {c && <CategoryIcon category={c} />}
      {label}
      <span className="text-subtle">{n}</span>
    </button>
  );
  return (
    <div role="group" aria-label="Show sections of" className="mb-3 flex flex-wrap items-center gap-1.5">
      {counts.size > 1 && (
        <>
          {button(null, "All", total)}
          {[...counts].map(([c, n]) => button(c, CATEGORY_UI[c].label, n))}
        </>
      )}
      {high && (
        <button aria-pressed={high.on} onClick={() => high.onChange(!high.on)} className={cn(chipClass(high.on), counts.size > 1 && "ml-2")}>
          <Signal aria-hidden className="size-3.5 shrink-0 text-destructive" />
          High<span className="sr-only"> importance</span>
          <span className="text-subtle">{high.count}</span>
        </button>
      )}
    </div>
  );
}
