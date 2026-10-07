import { Check } from "lucide-react";
import { type Category, type GuideSection, sectionBadge } from "@/lib/review/guide";
import { cn } from "@/lib/utils";
import { CarefulBadge, CATEGORY_UI, CategoryIcon } from "./categories";

interface Item {
  n: number;
  section: GuideSection;
  done: boolean;
}

/** One section in the navigator: its category, number and title, flagged when it needs care and checked once reviewed. */
function NavItem({ n, section: s, done, chip, onGo }: Item & { chip: boolean; onGo: (n: number) => void }) {
  return (
    <button
      onClick={() => onGo(n)}
      title={`${CATEGORY_UI[s.category].label}: ${s.title || "Untitled"}${done ? " (reviewed)" : ""}`}
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
      {s.critical && <CarefulBadge short />}
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

/** "All · Security 2 · Tests 3": a button a category, the one picked showing only its sections. */
export function CategoryFilter({ counts, total, value, onChange }: { counts: Map<Category, number>; total: number; value: Category | null; onChange: (c: Category | null) => void }) {
  const button = (c: Category | null, label: string, n: number) => (
    <button
      key={c ?? "all"}
      aria-pressed={value === c}
      onClick={() => onChange(c)}
      className={cn(
        "flex h-6 shrink-0 items-center gap-1.5 rounded-md border px-2 text-[12px] outline-none focus-visible:ring-1 focus-visible:ring-ring",
        value === c ? "border-border-strong bg-active text-foreground" : "border-border text-muted-foreground hover:bg-hover hover:text-foreground",
      )}
    >
      {c && <CategoryIcon category={c} />}
      {label}
      <span className="text-subtle">{n}</span>
    </button>
  );
  return (
    <div role="group" aria-label="Show sections of" className="mb-3 flex flex-wrap items-center gap-1.5">
      {button(null, "All", total)}
      {[...counts].map(([c, n]) => button(c, CATEGORY_UI[c].label, n))}
    </div>
  );
}
