import { cn } from "@/lib/utils";

/** A terminal rang or sent a notification and hasn't been looked at since: on its tab and its worktree. Read out where its row is read. */
export function NeedsYouDot({ className, ...props }: React.ComponentProps<"span">) {
  return <span role="img" aria-label="Needs you" {...props} className={cn("size-1.5 shrink-0 rounded-full bg-primary", className)} />;
}

/** An agent in a terminal is working: a ring where the needs-you dot fills in once it's done. Still, not pulsing: an animation would keep the window drawing. */
export function WorkingDot({ className, ...props }: React.ComponentProps<"span">) {
  return <span role="img" aria-label="Agent working" {...props} className={cn("size-1.5 shrink-0 rounded-full border border-muted-foreground", className)} />;
}
