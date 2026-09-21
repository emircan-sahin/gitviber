import type * as React from "react";
import * as ResizablePrimitive from "react-resizable-panels";
import { cn } from "@/lib/utils";

function ResizablePanelGroup({ className, ...props }: React.ComponentProps<typeof ResizablePrimitive.Group>) {
  return <ResizablePrimitive.Group className={cn("flex h-full w-full aria-[orientation=vertical]:flex-col", className)} {...props} />;
}

const ResizablePanel = ResizablePrimitive.Panel;

/** A 1px line with a wide invisible grab area; lights up teal while hovered or dragged. */
function ResizableHandle({ className, ...props }: React.ComponentProps<typeof ResizablePrimitive.Separator>) {
  return (
    <ResizablePrimitive.Separator
      className={cn(
        "relative w-px shrink-0 bg-transparent transition-colors after:absolute after:inset-y-0 after:left-1/2 after:w-2 after:-translate-x-1/2 hover:bg-primary/60 data-[separator=active]:bg-primary focus-visible:outline-none",
        className,
      )}
      {...props}
    />
  );
}

export { ResizableHandle, ResizablePanel, ResizablePanelGroup };
