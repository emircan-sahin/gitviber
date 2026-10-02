import { Bug, Check, ChevronRight, CircleCheck, CircleHelp, ClipboardList, Flame, Info, Lightbulb, List, type LucideIcon, MessageSquareWarning, OctagonAlert, Pencil, Quote, TriangleAlert, X, Zap } from "lucide-react";
import { Children, isValidElement, type ReactNode, useState } from "react";
import { useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";

type Rgb = [dark: string, light: string];
const BLUE: Rgb = ["2 122 255", "8 109 221"];
const CYAN: Rgb = ["83 223 221", "0 191 188"];
const GREEN: Rgb = ["68 207 110", "8 185 78"];
const ORANGE: Rgb = ["233 151 63", "236 117 0"];
const RED: Rgb = ["251 70 76", "233 49 71"];
const PURPLE: Rgb = ["168 130 255", "120 82 238"];
const GRAY: Rgb = ["158 158 158", "158 158 158"];

// Obsidian's callout types with their default icons and colors (aliases are folded in by the
// parser), then GitHub's five alerts with GitHub's.
const KINDS: Record<string, { icon: LucideIcon; color: Rgb }> = {
  note: { icon: Pencil, color: BLUE },
  abstract: { icon: ClipboardList, color: CYAN },
  info: { icon: Info, color: BLUE },
  todo: { icon: CircleCheck, color: BLUE },
  tip: { icon: Flame, color: CYAN },
  success: { icon: Check, color: GREEN },
  question: { icon: CircleHelp, color: ORANGE },
  warning: { icon: TriangleAlert, color: ORANGE },
  failure: { icon: X, color: RED },
  danger: { icon: Zap, color: RED },
  bug: { icon: Bug, color: RED },
  example: { icon: List, color: PURPLE },
  quote: { icon: Quote, color: GRAY },
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
  const kind = KINDS[type] ?? KINDS.note;
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
