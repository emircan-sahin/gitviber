import type * as React from "react";
import { cn } from "@/lib/utils";

function Input({ className, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      className={cn(
        "h-7 w-full rounded-md border border-border-strong bg-background px-2.5 text-[12px] text-foreground placeholder:text-subtle outline-none transition-[border-color,box-shadow] focus:border-primary select-text",
        className,
      )}
      {...props}
    />
  );
}

export { Input };
