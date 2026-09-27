import { CircleCheck, CircleDot, CircleX } from "lucide-react";
import { Tip } from "@/components/ui/tooltip";
import type { CiState } from "@/lib/api";
import { cn } from "@/lib/utils";

const LOOK: Record<CiState, [typeof CircleCheck, string, string]> = {
  success: [CircleCheck, "text-added", "Checks passed"],
  failure: [CircleX, "text-removed", "Checks failed"],
  pending: [CircleDot, "text-modified", "Checks running"],
};

export const ciLabel = (state: CiState) => LOOK[state][2];

/** CI's verdict on a commit, as a small icon; nothing when it has no checks. `bare`: inside a control whose own tooltip says it. */
export function CiBadge({ state, className, bare }: { state?: CiState; className?: string; bare?: boolean }) {
  if (!state) return null;
  const [Icon, color, label] = LOOK[state];
  const icon = <Icon aria-label={label} className={cn("size-3 shrink-0", color, className)} />;
  return bare ? icon : <Tip label={label}>{icon}</Tip>;
}
