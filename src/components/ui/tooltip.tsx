import { Tooltip as TooltipPrimitive } from "radix-ui";
import { cloneElement, isValidElement } from "react";
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

type Named = { "aria-label"?: string; "aria-labelledby"?: string; children?: React.ReactNode };

const hasText = (node: React.ReactNode): boolean =>
  typeof node === "string" || typeof node === "number"
    ? String(node).trim() !== ""
    : Array.isArray(node)
      ? node.some(hasText)
      : isValidElement<Named>(node) && hasText(node.props.children);

/**
 * An icon-only trigger takes its tooltip as its accessible name, or VoiceOver reads just "button".
 * One showing text keeps that text (what Voice Control users say), and a plain div/span wrapper
 * isn't the control: whatever it wraps names itself.
 */
function named(child: React.ReactNode, label: string) {
  if (!isValidElement<Named>(child) || child.type === "div" || child.type === "span") return child;
  const { props } = child;
  return props["aria-label"] || props["aria-labelledby"] || hasText(props.children) ? child : cloneElement(child, { "aria-label": label });
}

/** Small helper: wraps any trigger with a tooltip label (and optional shortcut). */
function Tip({ label, shortcut, children, side }: { label: string; shortcut?: string; children: React.ReactNode; side?: "top" | "bottom" | "left" | "right" }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>{named(children, label)}</TooltipTrigger>
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
function DisabledTip({ label, shortcut, disabled, className, children }: { label: string; shortcut?: string; disabled: boolean; className?: string; children: React.ReactNode }) {
  return (
    <Tip label={label} shortcut={shortcut}>
      <span tabIndex={disabled ? 0 : undefined} className={cn("rounded-md outline-none focus-visible:ring-1 focus-visible:ring-ring", className)}>
        {children}
      </span>
    </Tip>
  );
}

export { DisabledTip, Tip, Tooltip, TooltipContent, TooltipProvider, TooltipTrigger };
