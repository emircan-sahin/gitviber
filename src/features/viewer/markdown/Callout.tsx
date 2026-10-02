import { Bug, Check, ChevronRight, CircleCheck, CircleHelp, ClipboardList, Flame, Info, Lightbulb, List, type LucideIcon, MessageSquareWarning, OctagonAlert, Pencil, Quote, TriangleAlert, X, Zap } from "lucide-react";
import { Children, isValidElement, type ReactNode, useState } from "react";
import { COLORS, type Rgb } from "@/lib/obsidian/colors";
import { useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";

// Obsidian's callout types with their default icons and colors (aliases are folded in by the
// parser), then GitHub's five alerts with GitHub's.
const KINDS: Record<string, { icon: LucideIcon; color: Rgb }> = {
  note: { icon: Pencil, color: COLORS.blue },
  abstract: { icon: ClipboardList, color: COLORS.cyan },
  info: { icon: Info, color: COLORS.blue },
  todo: { icon: CircleCheck, color: COLORS.blue },
  tip: { icon: Flame, color: COLORS.cyan },
  success: { icon: Check, color: COLORS.green },
  question: { icon: CircleHelp, color: COLORS.orange },
  warning: { icon: TriangleAlert, color: COLORS.orange },
  failure: { icon: X, color: COLORS.red },
  danger: { icon: Zap, color: COLORS.red },
  bug: { icon: Bug, color: COLORS.red },
  example: { icon: List, color: COLORS.purple },
  quote: { icon: Quote, color: COLORS.gray },
  "gh-note": { icon: Info, color: ["68 147 248", "9 105 218"] },
  "gh-tip": { icon: Lightbulb, color: ["63 185 80", "26 127 55"] },
  "gh-important": { icon: MessageSquareWarning, color: ["171 125 248", "130 80 223"] },
  "gh-warning": { icon: TriangleAlert, color: ["210 153 34", "154 103 0"] },
  "gh-caution": { icon: OctagonAlert, color: ["248 81 73", "209 36 47"] },
};

/**
 * A callout (Obsidian) or alert (GitHub): its title, then the body, folded away when it was
 * written with "-" and foldable with "+". `children` are the title's and the body's divs.
 */
export function Callout({ type, fold, children }: { type: string; fold: string; children: ReactNode }) {
  const { dark } = useSettings();
  const [open, setOpen] = useState(fold !== "-");
  // Raw HTML can name any type, "toString" too.
  const kind = Object.hasOwn(KINDS, type) ? KINDS[type] : KINDS.note;
  const [title, ...body] = Children.toArray(children).filter(isValidElement);
  const Icon = kind.icon;
  const github = type.startsWith("gh-");
  const header = (
    <>
      <Icon className="size-4 shrink-0" />
      <div className="callout-title-text">{title}</div>
      {fold && <ChevronRight className={cn("size-4 shrink-0 transition-transform", open && "rotate-90")} />}
    </>
  );
  return (
    <div className={github ? "callout callout-github" : "callout"} style={{ "--callout": kind.color[dark ? 0 : 1] } as React.CSSProperties}>
      {fold ? (
        <button type="button" aria-expanded={open} onClick={() => setOpen(!open)} className="callout-title w-full text-left">
          {header}
        </button>
      ) : (
        <div className="callout-title">{header}</div>
      )}
      {open && body}
    </div>
  );
}
