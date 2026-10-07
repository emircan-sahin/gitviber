import { CircleCheck, CircleDot, CircleX } from "lucide-react";
import { Tip, Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { CiState } from "@/lib/api";
import { knownSummary } from "@/lib/github/commitChecks";
import { cn } from "@/lib/utils";

const LOOK: Record<CiState, [typeof CircleCheck, string, string]> = {
  success: [CircleCheck, "text-added", "Checks passed"],
  failure: [CircleX, "text-removed", "Checks failed"],
  pending: [CircleDot, "text-modified", "Checks running"],
};

export const ciLabel = (state: CiState) => LOOK[state][2];

/**
 * CI's verdict on a commit, as a small icon; nothing when it has no checks. `bare`: inside a control
 * whose own tooltip says it. `url`: the commit's GitHub page, whose checks the tooltip counts if its
 * header in the viewer has read them.
 */
export function CiBadge({ state, className, bare, url }: { state?: CiState; className?: string; bare?: boolean; url?: string }) {
  if (!state) return null;
  const [Icon, color, label] = LOOK[state];
  const icon = <Icon aria-label={label} className={cn("size-3 shrink-0", color, className)} />;
  if (bare) return icon;
  if (!url) return <Tip label={label}>{icon}</Tip>;
  return (
    <Tooltip>
      <TooltipTrigger asChild>{icon}</TooltipTrigger>
      <TooltipContent>
        <Counted url={url} state={state} label={label} />
      </TooltipContent>
    </Tooltip>
  );
}

// Rendered as the tooltip opens, so it reads the header's latest answer; asks for nothing.
function Counted({ url, state, label }: { url: string; state: CiState; label: string }) {
  const summary = knownSummary(url, state);
  return summary ? `${label}: ${summary}` : label;
}
