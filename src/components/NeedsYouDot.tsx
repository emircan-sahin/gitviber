import { cn } from "@/lib/utils";

/** A terminal rang or sent a notification and hasn't been looked at since: on its tab and its worktree. */
export function NeedsYouDot({ className, ...props }: React.ComponentProps<"span">) {
  return <span aria-hidden {...props} className={cn("size-1.5 shrink-0 rounded-full bg-primary", className)} />;
}
