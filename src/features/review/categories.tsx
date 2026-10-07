import { BookOpen, Cpu, Database, FlaskConical, Hammer, Image, Languages, Lightbulb, type LucideIcon, Package, Palette, Plug, ScrollText, Settings2, Shapes, ShieldAlert, SquareTerminal } from "lucide-react";
import type { Category } from "@/lib/review/guide";
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

/** A section or note to review carefully: security, data loss, hard to revert. */
export function CarefulBadge({ short = false }: { short?: boolean }) {
  return (
    <span title="Review carefully" className="inline-flex shrink-0 items-center gap-1 text-[11.5px] font-medium text-destructive">
      <ShieldAlert aria-hidden className="size-3.5 shrink-0" />
      {short ? <span className="sr-only">Review carefully</span> : "Review carefully"}
    </span>
  );
}
