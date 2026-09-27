import { Tooltip as TooltipPrimitive } from "radix-ui";
import type * as React from "react";
import { cn } from "@/lib/utils";
import { WINDOW_SAFE_AREA } from "./dropdown-menu";

// Tooltips here are labels, never hovered into. Hoverable content keeps one open while the pointer
// crosses toward it, and a neighbouring button under that path couldn't open its own.
function TooltipProvider({ delayDuration = 300, disableHoverableContent = true, ...props }: React.ComponentProps<typeof TooltipPrimitive.Provider>) {
  return <TooltipPrimitive.Provider delayDuration={delayDuration} disableHoverableContent={disableHoverableContent} {...props} />;
}

const Tooltip = TooltipPrimitive.Root;
const TooltipTrigger = TooltipPrimitive.Trigger;

function TooltipContent({ className, sideOffset = 6, collisionPadding = WINDOW_SAFE_AREA, children, ...props }: React.ComponentProps<typeof TooltipPrimitive.Content>) {
  return (
    <TooltipPrimitive.Portal>
      <TooltipPrimitive.Content
        sideOffset={sideOffset}
        collisionPadding={collisionPadding}
        className={cn(
          "z-50 max-w-96 rounded-md border border-border-strong bg-elevated px-1.5 py-0.5 text-[11.5px] whitespace-pre-line text-foreground shadow-md shadow-black/40 animate-in fade-in-0 zoom-in-95",
          className,
        )}
        {...props}
      >
        {children}
      </TooltipPrimitive.Content>
    </TooltipPrimitive.Portal>
  );
}

/** Small helper: wraps any trigger with a tooltip label (and optional shortcut). */
function Tip({ label, shortcut, children, side }: { label: string; shortcut?: string; children: React.ReactNode; side?: "top" | "bottom" | "left" | "right" }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>{children}</TooltipTrigger>
      <TooltipContent side={side}>
        {label}
        {shortcut && <span className="ml-2 font-mono text-[11px] text-subtle">{shortcut}</span>}
      </TooltipContent>
    </Tooltip>
  );
}

/**
 * `Tip` around a control that may be disabled, which takes no pointer or focus then: the wrapper
 * shows the label instead, and takes the focus (`disabled`) so the keyboard gets the reason too.
 */
function DisabledTip({ label, disabled, className, children }: { label: string; disabled: boolean; className?: string; children: React.ReactNode }) {
  return (
    <Tip label={label}>
      <span tabIndex={disabled ? 0 : undefined} className={cn("rounded-md outline-none focus-visible:ring-1 focus-visible:ring-ring", className)}>
        {children}
      </span>
    </Tip>
  );
}

export { DisabledTip, Tip, Tooltip, TooltipContent, TooltipProvider, TooltipTrigger };
