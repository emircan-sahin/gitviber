import { MessageSquareText } from "lucide-react";
import { cn } from "@/lib/utils";
import { CarefulBadge } from "./categories";

/** A guided review's note on a file or a line of its diff, as the agent wrote it. */
export function GuideNote({ text, critical }: { text: string; critical: boolean }) {
  return (
    <div className={cn("my-0.5 flex max-w-3xl items-start gap-2 rounded-md border bg-panel px-2.5 py-1.5 font-sans text-[12px] leading-snug", critical ? "border-destructive/60" : "border-border")}>
      <MessageSquareText aria-hidden className="mt-0.5 size-3.5 shrink-0 text-subtle" />
      <div className="min-w-0 flex-1 whitespace-pre-wrap select-text">
        {critical && (
          <>
            <CarefulBadge />{" "}
          </>
        )}
        <span className="sr-only">Guide note: </span>
        {text}
      </div>
    </div>
  );
}
