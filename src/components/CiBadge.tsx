import { CircleCheck, CircleDot, CircleX } from "lucide-react";
import { Tip } from "@/components/ui/tooltip";
import type { CiState } from "@/lib/api";
import { cn } from "@/lib/utils";

const LOOK: Record<CiState, [typeof CircleCheck, string, string]> = {
  success: [CircleCheck, "text-added", "Checks passed"],
  failure: [CircleX, "text-removed", "Checks failed"],
  pending: [CircleDot, "text-modified", "Checks running"],
};

/** CI's verdict on a commit, as a small icon; nothing when it has no checks. */
export function CiBadge({ state, className }: { state?: CiState; className?: string }) {
  if (!state) return null;
  const [Icon, color, label] = LOOK[state];
  return (
    <Tip label={label}>
      <Icon aria-label={label} className={cn("size-3 shrink-0", color, className)} />
    </Tip>
  );
}
