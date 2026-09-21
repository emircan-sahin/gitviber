import { ChevronRightIcon } from "lucide-react";
import { ContextMenu as ContextMenuPrimitive } from "radix-ui";
import type * as React from "react";
import { createContext, useContext, useState } from "react";
import { cn } from "@/lib/utils";
import { WINDOW_SAFE_AREA } from "./dropdown-menu";

// Distance from the cursor down to the bottom of the right-clicked row. Radix opens the menu at
// the cursor, which puts it over the row's own label; this drops it just below the row instead.
const RowOffset = createContext<[number, (offset: number) => void]>([0, () => {}]);

// Non-modal: see DropdownMenu.
function ContextMenu({ modal = false, ...props }: React.ComponentProps<typeof ContextMenuPrimitive.Root>) {
  const offset = useState(0);
  return (
    <RowOffset.Provider value={offset}>
      <ContextMenuPrimitive.Root modal={modal} {...props} />
    </RowOffset.Provider>
  );
}

/** Rows are the `role="button"` elements inside the trigger; empty space keeps the menu at the cursor. */
function ContextMenuTrigger({ onContextMenu, ...props }: React.ComponentProps<typeof ContextMenuPrimitive.Trigger>) {
  const [, setOffset] = useContext(RowOffset);
  return (
    <ContextMenuPrimitive.Trigger
      {...props}
      onContextMenu={(ev) => {
        onContextMenu?.(ev);
        const row = (ev.target as HTMLElement).closest('[role="button"]');
        setOffset(row && ev.currentTarget.contains(row) ? row.getBoundingClientRect().bottom - ev.clientY : 0);
      }}
    />
  );
}

const ContextMenuGroup = ContextMenuPrimitive.Group;
const ContextMenuSub = ContextMenuPrimitive.Sub;

function ContextMenuContent({ className, collisionPadding = WINDOW_SAFE_AREA, ...props }: React.ComponentProps<typeof ContextMenuPrimitive.Content>) {
  const [offset] = useContext(RowOffset);
  return (
    <ContextMenuPrimitive.Portal>
      <ContextMenuPrimitive.Content
        alignOffset={offset}
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

function ContextMenuSubTrigger({ className, children, ...props }: React.ComponentProps<typeof ContextMenuPrimitive.SubTrigger>) {
  return (
    <ContextMenuPrimitive.SubTrigger
      className={cn(
        "relative flex cursor-pointer items-center gap-2 rounded-sm px-2 py-1 text-[12px] outline-none select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-40 data-[highlighted]:bg-primary data-[highlighted]:text-primary-foreground data-[highlighted]:[&_svg]:text-primary-foreground data-[state=open]:bg-hover [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-muted-foreground",
        className,
      )}
      {...props}
    >
      {children}
      <ChevronRightIcon className="ml-auto" />
    </ContextMenuPrimitive.SubTrigger>
  );
}

function ContextMenuSubContent({ className, collisionPadding = WINDOW_SAFE_AREA, ...props }: React.ComponentProps<typeof ContextMenuPrimitive.SubContent>) {
  return (
    <ContextMenuPrimitive.Portal>
      <ContextMenuPrimitive.SubContent
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

function ContextMenuLabel({ className, ...props }: React.ComponentProps<typeof ContextMenuPrimitive.Label>) {
  return <ContextMenuPrimitive.Label className={cn("px-2 pt-2 pb-1 text-[11px] font-medium tracking-wide text-subtle uppercase", className)} {...props} />;
}

function ContextMenuSeparator({ className, ...props }: React.ComponentProps<typeof ContextMenuPrimitive.Separator>) {
  return <ContextMenuPrimitive.Separator className={cn("-mx-1 my-1 h-px bg-border", className)} {...props} />;
}

function ContextMenuShortcut({ className, ...props }: React.ComponentProps<"span">) {
  return <span className={cn("ml-auto pl-4 font-mono text-[11px] text-subtle in-data-[highlighted]:text-primary-foreground/70", className)} {...props} />;
}

export {
  ContextMenu,
  ContextMenuContent,
  ContextMenuGroup,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
};
