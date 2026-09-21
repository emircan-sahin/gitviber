import { Dialog as DialogPrimitive } from "radix-ui";
import type * as React from "react";
import { cn } from "@/lib/utils";

const Dialog = DialogPrimitive.Root;
const DialogClose = DialogPrimitive.Close;

function DialogContent({ className, children, ...props }: React.ComponentProps<typeof DialogPrimitive.Content>) {
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-black/55 animate-in fade-in-0" />
      {/* Global shortcuts stand down while this is open (see keybindings.ts). */}
      <DialogPrimitive.Content
        data-modal=""
        className={cn(
          "fixed top-[22%] left-1/2 z-50 w-full max-w-md -translate-x-1/2 rounded-md border border-border-strong bg-elevated p-5 shadow-lg shadow-black/60 animate-in fade-in-0 zoom-in-95",
          className,
        )}
        {...props}
      >
        {children}
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  );
}

function DialogTitle({ className, ...props }: React.ComponentProps<typeof DialogPrimitive.Title>) {
  return <DialogPrimitive.Title className={cn("text-[13.5px] font-semibold", className)} {...props} />;
}

function DialogDescription({ className, ...props }: React.ComponentProps<typeof DialogPrimitive.Description>) {
  return <DialogPrimitive.Description className={cn("mt-1 text-[12px] text-muted-foreground", className)} {...props} />;
}

export { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle };
