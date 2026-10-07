import { LOOK_LABEL, type Look } from "@/lib/terminal/agentLook";
import { cn } from "@/lib/utils";

/** Each look's color (theme tokens), shared with text that names it. */
export const LOOK_TEXT: Record<Look, string> = { needs: "text-destructive", unread: "text-info", working: "text-modified", done: "text-muted-foreground" };

// Drawn in currentColor, so a highlighted row turns any look to its ink with one text class. A
// question is the loudest: the red of the Dock badge it counts toward, with a halo for its shape.
const SHAPE: Record<Look, string> = { needs: "bg-current ring-2 ring-current/30", unread: "bg-current", working: "bg-current agent-working", done: "border border-current" };

/** A terminal's or an agent's state (agentLook) as a dot; nothing for none. */
export function StatusDot({ look, className, ...props }: { look: Look | null } & React.ComponentProps<"span">) {
  if (!look) return null;
  return <span role="img" aria-label={LOOK_LABEL[look]} {...props} className={cn("size-1.5 shrink-0 rounded-full", SHAPE[look], LOOK_TEXT[look], className)} />;
}
