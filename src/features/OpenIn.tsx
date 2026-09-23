import { Check, ChevronDown, Settings2, SquareArrowOutUpRight } from "lucide-react";
import { Fragment, useState } from "react";
import { ContextMenuItem, ContextMenuLabel, ContextMenuSub, ContextMenuSubContent, ContextMenuSubTrigger } from "@/components/ui/context-menu";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tip } from "@/components/ui/tooltip";
import { useCommands, useShortcut } from "@/lib/keybindings";
import { GROUPS, type OpenApp, type OpenTarget, openIn, refreshOpenApps, useOpenApps } from "@/lib/openIn";
import { lineInView } from "./MonacoView";
import { openSettings } from "./SettingsDialog";

const grouped = (apps: OpenApp[]) => GROUPS.map(([group, label]) => [label, apps.filter((a) => a.group === group)] as const).filter(([, list]) => list.length);

/**
 * The status bar's split button: a click opens `target()` in the last app picked, the arrow
 * lists the rest. Also runs the rebindable "Open in" command, which shows the list until an
 * app has been picked.
 */
export function OpenInButton({ target }: { target: () => OpenTarget }) {
  const { apps, last } = useOpenApps();
  const [open, setOpen] = useState(false);
  const shortcut = useShortcut("file.openIn");
  // Apps come and go: look again each time the list opens, showing the last answer meanwhile.
  const show = (on: boolean) => {
    setOpen(on);
    if (on) refreshOpenApps();
  };
  const run = () => (last ? openIn(last, target()) : show(true));
  useCommands({ "file.openIn": run });
  return (
    <div className="flex items-center">
      <Tip label={last ? `Open in ${last.name}` : "Open in…"} shortcut={shortcut}>
        <button onClick={run} className="flex items-center gap-1 hover:text-foreground">
          <SquareArrowOutUpRight className="size-3" />
          {last ? last.name : "Open in…"}
        </button>
      </Tip>
      <DropdownMenu open={open} onOpenChange={show}>
        <DropdownMenuTrigger asChild>
          <button aria-label="Open in…" className="flex items-center pl-0.5 hover:text-foreground">
            <ChevronDown className="size-3" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent side="top" align="end">
          {grouped(apps).map(([label, list], i) => (
            <Fragment key={label}>
              {i > 0 && <DropdownMenuSeparator />}
              <DropdownMenuLabel>{label}</DropdownMenuLabel>
              {list.map((a) => (
                <DropdownMenuItem key={a.id} onSelect={() => openIn(a, target())}>
                  {a.name}
                  {a.id === last?.id && <Check className="ml-auto" />}
                </DropdownMenuItem>
              ))}
            </Fragment>
          ))}
          {!apps.length && <DropdownMenuItem disabled>No known editors or terminals found</DropdownMenuItem>}
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={() => openSettings("openIn")}>
            <Settings2 /> Add Your Own App…
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

/** A right-click menu's "Open in <last app>" for `path`; a submenu of every app until one is picked. */
export function OpenInMenuItem({ path, disabled }: { path: string; disabled?: boolean }) {
  const { apps, last } = useOpenApps();
  const target = () => ({ path, line: lineInView(path) });
  if (last)
    return (
      <ContextMenuItem disabled={disabled} onSelect={() => openIn(last, target())}>
        <SquareArrowOutUpRight /> Open in {last.name}
      </ContextMenuItem>
    );
  if (!apps.length) return null;
  return (
    <ContextMenuSub>
      <ContextMenuSubTrigger disabled={disabled}>
        <SquareArrowOutUpRight /> Open in
      </ContextMenuSubTrigger>
      <ContextMenuSubContent>
        {grouped(apps).map(([label, list]) => (
          <Fragment key={label}>
            <ContextMenuLabel>{label}</ContextMenuLabel>
            {list.map((a) => (
              <ContextMenuItem key={a.id} onSelect={() => openIn(a, target())}>
                {a.name}
              </ContextMenuItem>
            ))}
          </Fragment>
        ))}
      </ContextMenuSubContent>
    </ContextMenuSub>
  );
}
