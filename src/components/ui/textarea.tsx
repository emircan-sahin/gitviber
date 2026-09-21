import type * as React from "react";
import { cn } from "@/lib/utils";

function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      className={cn(
        "w-full resize-none rounded-md border border-border-strong bg-background px-3 py-2 text-[12px] leading-relaxed text-foreground placeholder:text-subtle outline-none transition-[border-color,box-shadow] focus:border-primary select-text",
        className,
      )}
      {...props}
    />
  );
}

export { Textarea };
