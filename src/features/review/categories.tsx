import { BookOpen, Cpu, Database, FlaskConical, Hammer, Image, Languages, Lightbulb, type LucideIcon, Package, Palette, Plug, ScrollText, Settings2, Shapes, ShieldAlert, Signal, SignalLow, SignalMedium, SquareTerminal } from "lucide-react";
import type { Category, Importance } from "@/lib/review/guide";
import { cn } from "@/lib/utils";

/** How a guide's section categories show: always an icon and a name, the color only on top. */
export const CATEGORY_UI: Record<Category, { label: string; icon: LucideIcon; tone: string }> = {
  ui: { label: "UI", icon: Palette, tone: "text-info" },
  api: { label: "API", icon: Plug, tone: "text-renamed" },
  core: { label: "Core", icon: Cpu, tone: "text-primary" },
  data: { label: "Data", icon: Database, tone: "text-modified" },
  cli: { label: "CLI", icon: SquareTerminal, tone: "text-renamed" },
  security: { label: "Security", icon: ShieldAlert, tone: "text-destructive" },
  tests: { label: "Tests", icon: FlaskConical, tone: "text-added" },
  docs: { label: "Docs", icon: BookOpen, tone: "text-muted-foreground" },
  examples: { label: "Examples", icon: Lightbulb, tone: "text-muted-foreground" },
  deps: { label: "Dependencies", icon: Package, tone: "text-modified" },
  build: { label: "Build", icon: Hammer, tone: "text-conflict" },
  scripts: { label: "Scripts", icon: ScrollText, tone: "text-conflict" },
  config: { label: "Config", icon: Settings2, tone: "text-info" },
  i18n: { label: "Translations", icon: Languages, tone: "text-info" },
  assets: { label: "Assets", icon: Image, tone: "text-renamed" },
  other: { label: "Other", icon: Shapes, tone: "text-subtle" },
};

export function CategoryIcon({ category, className }: { category: Category; className?: string }) {
  const { icon: Icon, tone } = CATEGORY_UI[category];
  return <Icon aria-hidden className={cn("size-3.5 shrink-0", tone, className)} />;
}

export function CategoryTag({ category }: { category: Category }) {
  return (
    <span className="inline-flex shrink-0 items-center gap-1 text-[11.5px] text-muted-foreground">
      <CategoryIcon category={category} />
      {CATEGORY_UI[category].label}
    </span>
  );
}

/** A note to review carefully: security, data loss, hard to revert. */
export function CarefulBadge() {
  return (
    <span className="inline-flex shrink-0 items-center gap-1 text-[11.5px] font-medium text-destructive">
      <ShieldAlert aria-hidden className="size-3.5 shrink-0" />
      Review carefully
    </span>
  );
}

/** How carefully a section wants reading, as the guide shows it. */
export const IMPORTANCE_UI: Record<Importance, { label: string; icon: LucideIcon; tone: string; tip: string }> = {
  high: { label: "High", icon: Signal, tone: "font-medium text-destructive", tip: "Review carefully: security, data loss, or hard to revert or get right" },
  medium: { label: "Medium", icon: SignalMedium, tone: "text-muted-foreground", tip: "A change in behavior worth a careful read" },
  low: { label: "Low", icon: SignalLow, tone: "text-subtle", tip: "Mechanical, docs or style: a skim is enough" },
};

/** Always its word (only to screen readers when `short`, which leaves the tooltip to its row), the color only on top. */
export function ImportanceTag({ importance, short = false }: { importance: Importance; short?: boolean }) {
  const { label, icon: Icon, tone, tip } = IMPORTANCE_UI[importance];
  return (
    <span title={short ? undefined : `${label} importance. ${tip}`} className={cn("inline-flex shrink-0 items-center gap-1 text-[11.5px]", tone)}>
      <Icon aria-hidden className="size-3.5 shrink-0" />
      {short ? <span className="sr-only">, {label} importance</span> : (
        <>
          {label}
          <span className="sr-only"> importance</span>
        </>
      )}
    </span>
  );
}
