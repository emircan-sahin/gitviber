import { ContextMenu as ContextMenuPrimitive } from "radix-ui";
import type * as React from "react";
import { cn } from "@/lib/utils";
import { WINDOW_SAFE_AREA } from "./dropdown-menu";

const ContextMenu = ContextMenuPrimitive.Root;
const ContextMenuTrigger = ContextMenuPrimitive.Trigger;

function ContextMenuContent({ className, collisionPadding = WINDOW_SAFE_AREA, ...props }: React.ComponentProps<typeof ContextMenuPrimitive.Content>) {
  return (
    <ContextMenuPrimitive.Portal>
      <ContextMenuPrimitive.Content
        collisionPadding={collisionPadding}
        className={cn(
          "z-50 max-h-(--radix-context-menu-content-available-height) min-w-48 overflow-y-auto rounded-md border border-border-strong bg-elevated p-1 text-foreground shadow-lg shadow-black/50 animate-in fade-in-0 zoom-in-95",
          className,
        )}
        {...props}
      />
    </ContextMenuPrimitive.Portal>
  );
}

function ContextMenuItem({ className, ...props }: React.ComponentProps<typeof ContextMenuPrimitive.Item>) {
  return (
    <ContextMenuPrimitive.Item
      className={cn(
        "relative flex cursor-pointer items-center gap-2 rounded-sm px-2 py-1 text-[12px] outline-none select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-40 data-[highlighted]:bg-primary data-[highlighted]:text-primary-foreground data-[highlighted]:[&_svg]:text-primary-foreground [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-muted-foreground",
        className,
      )}
      {...props}
    />
  );
}

function ContextMenuSeparator({ className, ...props }: React.ComponentProps<typeof ContextMenuPrimitive.Separator>) {
  return <ContextMenuPrimitive.Separator className={cn("-mx-1 my-1 h-px bg-border", className)} {...props} />;
}

function ContextMenuShortcut({ className, ...props }: React.ComponentProps<"span">) {
  return <span className={cn("ml-auto pl-4 font-mono text-[11px] text-subtle in-data-[highlighted]:text-primary-foreground/70", className)} {...props} />;
}

export { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuShortcut, ContextMenuTrigger };
