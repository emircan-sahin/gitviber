import { ClipboardPaste, Columns2, Copy, Globe, Rows2, SquareTerminal, TextSelect, X } from "lucide-react";
import { Fragment, useLayoutEffect, useRef, useState } from "react";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "@/components/ui/context-menu";
import { useGroupRef } from "react-resizable-panels";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Tip } from "@/components/ui/tooltip";
import { type Layout } from "@/lib/terminal/layout";
import {
  attachPane,
  copyLastOutput,
  copyPaneSelection,
  equalizeSplit,
  focusActive,
  focusTerminalPane,
  killPane,
  paneDir,
  paneMenuState,
  panePty,
  paneTakesMouse,
  pasteIntoPane,
  renamePane,
  resizeSplit,
  selectLastOutput,
  splitActive,
  type TerminalGroup,
} from "@/lib/terminal/terminals";
import { cn } from "@/lib/utils";
import { createStore } from "@/lib/store";
import { folderName } from "@/lib/path";
import { NameInput } from "@/components/NameInput";
import { StatusDot } from "@/components/StatusDot";
import { lookLabel, paneLook } from "@/lib/terminal/agentLook";
import { portUrl, scanPorts, usePorts } from "@/lib/browser/ports";
import { askOpenPage } from "@/lib/browser/store";

/** A split's structure without its sizes: what a tab re-lays out on (a split or close), not a drag. */
export const shape = (l: Layout): string => (typeof l === "number" ? String(l) : `${l.dir}(${l.children.map(shape).join()})`);

type PaneInfo = TerminalGroup["panes"][number];

/**
 * A tab's panes as split; a dragged divider's sizes are kept in the tab's layout, by the split's
 * `path` from the top. `dim`: how far the panes other than the focused one fade.
 */
export function LayoutView({ group, node, focused, dim, path = [] }: { group: TerminalGroup; node: Layout; focused: number; dim: number; path?: number[] }) {
  const id = (i: number) => `pane-${[...path, i].join("-")}`;
  const groupRef = useGroupRef();
  const box = useRef<HTMLDivElement>(null);
  const sizes = typeof node === "number" ? "" : node.sizes.join();
  // Sizes set from outside (equalized): the group was laid out from them only as it mounted. A drag's
  // own sizes are already what it shows. With no size (the panel hidden), once it has one again.
  useLayoutEffect(() => {
    const g = groupRef.current;
    const el = box.current;
    if (!g || !el || typeof node === "number") return;
    const apply = () => {
      const now = g.getLayout();
      if (node.sizes.some((s, i) => Math.abs((now[id(i)] ?? s) - s) > 0.1)) g.setLayout(Object.fromEntries(node.sizes.map((s, i) => [id(i), s])));
    };
    if (el.offsetWidth && el.offsetHeight) return apply();
    const sized = new ResizeObserver(() => {
      if (!el.offsetWidth || !el.offsetHeight) return;
      sized.disconnect();
      apply();
    });
    sized.observe(el);
    return () => sized.disconnect();
  }, [sizes]);
  if (typeof node === "number") {
    // A pane in a split gets a header, as in cmux: what runs in each, and which one has the keys.
    const pane = path.length ? group.panes.find((p) => p.id === node) : undefined;
    return <PaneView id={node} dim={node === focused ? 0 : dim} header={pane && { pane, focused: node === focused }} />;
  }
  const row = node.dir === "row";
  return (
    <ResizablePanelGroup
      groupRef={groupRef}
      elementRef={box}
      orientation={row ? "horizontal" : "vertical"}
      defaultLayout={Object.fromEntries(node.sizes.map((size, i) => [id(i), size]))}
      onLayoutChanged={(layout, { isUserInteraction }) => isUserInteraction && resizeSplit(group.id, path, node.children.map((_, i) => layout[id(i)]))}
    >
      {node.children.map((c, i) => (
        <Fragment key={id(i)}>
          {/* The library focuses a divider as it's grabbed, from a few px either side of it too, where the
              pointer's release lands on the pane: the keys go back to the pane on any release. */}
          {/* Stronger than --border, which all but vanishes against a dark terminal. */}
          {i > 0 && (
            <ResizableHandle
              className="bg-foreground/20"
              onFocus={() => window.addEventListener("pointerup", focusActive, { capture: true, once: true })}
              disableDoubleClick
              onDoubleClick={() => equalizeSplit(group.id, path)}
            />
          )}
          <ResizablePanel id={id(i)} minSize={row ? 160 : 80}>
            <LayoutView group={group} node={c} focused={focused} dim={dim} path={[...path, i]} />
          </ResizablePanel>
        </Fragment>
      ))}
    </ResizablePanelGroup>
  );
}

/** `dim`: how far it fades into the panel's background, while another pane of its tab has focus. */
function PaneView({ id, dim, header }: { id: number; dim: number; header?: { pane: PaneInfo; focused: boolean } }) {
  const ref = useRef<HTMLDivElement>(null);
  // Layout effect: the pane is in place before paint, so focusing it next frame works.
  useLayoutEffect(() => attachPane(id, ref.current!), [id]);
  // Read as the menu opens: the selection and the last command change under it.
  const [can, setCan] = useState(() => paneMenuState(id));
  // What the pane's programs serve (a dev server): read again as the menu opens.
  const pty = panePty(id);
  const ports = usePorts(pty === null ? [] : [pty]);
  return (
    <div className="flex h-full flex-col">
      {header && <PaneHeader pane={header.pane} focused={header.focused} />}
      <ContextMenu
        onOpenChange={(open) => {
          if (!open) return;
          setCan(paneMenuState(id));
          if (pty !== null) void scanPorts([pty]);
        }}
      >
        <ContextMenuTrigger asChild>
          {/* Inset from the edges like the code view's text; the scrollbar keeps the right edge, command marks the left. */}
          <div ref={ref} className="min-h-0 w-full flex-1 pt-2 pb-1 pl-3 transition-opacity duration-150" style={{ opacity: 1 - dim }} onContextMenu={(e) => paneTakesMouse(id, e.nativeEvent) && e.preventDefault()} />
        </ContextMenuTrigger>
        <ContextMenuContent
          onCloseAutoFocus={(e) => {
            e.preventDefault();
            focusActive();
          }}
        >
          <ContextMenuItem disabled={!can.selection} onSelect={() => copyPaneSelection(id)}>
            <Copy /> Copy
          </ContextMenuItem>
          <ContextMenuItem disabled={!can.paste} onSelect={() => pasteIntoPane(id)}>
            <ClipboardPaste /> Paste
          </ContextMenuItem>
          <ContextMenuSeparator />
          {/* Shell integration marks where it starts and ends (Settings → Terminal). */}
          <ContextMenuItem disabled={!can.output} onSelect={() => copyLastOutput(id)}>
            <Copy /> Copy Last Command Output
          </ContextMenuItem>
          <ContextMenuItem disabled={!can.output} onSelect={() => selectLastOutput(id)}>
            <TextSelect /> Select Last Command Output
          </ContextMenuItem>
          {ports.length > 0 && <ContextMenuSeparator />}
          {ports.map((p) => (
            <ContextMenuItem key={p.port} onSelect={() => askOpenPage(portUrl(p.port))}>
              <Globe /> Open localhost:{p.port}
            </ContextMenuItem>
          ))}
        </ContextMenuContent>
      </ContextMenu>
    </div>
  );
}

/** The split pane whose header shows its name field (⌘R, or a double-click on the header). */
export const renamingPane = createStore<number | null>(null);

/** A split pane's title bar: the user's name for it, else the program's title, else its folder; and its own split and kill. */
function PaneHeader({ pane, focused }: { pane: PaneInfo; focused: boolean }) {
  const id = pane.id;
  const renaming = renamingPane.use() === id;
  const title = pane.name ?? (pane.title || folderName(paneDir(id) ?? pane.cwd));
  const look = paneLook(pane);
  const action = (label: string, Icon: typeof X, run: () => void) => (
    <Tip label={label}>
      <button
        aria-label={label}
        tabIndex={-1}
        onClick={(e) => {
          e.stopPropagation();
          // A double-click's second click would split again (splitActive is async, the first not done yet).
          if (e.detail < 2) run();
        }}
        className="flex size-5 items-center justify-center rounded-sm text-subtle hover:bg-active hover:text-foreground"
      >
        <Icon className="size-3" />
      </button>
    </Tip>
  );
  return (
    <div
      // The keys stay with the terminal: a click here gives them to this pane.
      onMouseDown={(e) => !(e.target instanceof HTMLInputElement) && e.preventDefault()}
      onClick={() => focusTerminalPane(id)}
      // Not on its buttons: a double-click on Split is still a split, not a rename.
      onDoubleClick={(e) => !(e.target as Element).closest("button") && renamingPane.set(id)}
      className="group/pane relative flex h-6 shrink-0 cursor-default items-center gap-1.5 border-b border-border bg-panel pr-1 pl-2.5 text-[11.5px] select-none"
    >
      {focused && <span className="absolute inset-x-0 top-0 h-0.5 bg-primary" />}
      <SquareTerminal className={cn("size-3 shrink-0", focused ? "text-primary" : "text-subtle")} />
      {renaming ? (
        <NameInput
          initial={title}
          onDone={(name, refocus) => {
            renamingPane.set(null);
            // Left as it was, the title isn't a name the user gave.
            if (name !== null && name.trim() !== title) renamePane(id, name);
            if (refocus) focusTerminalPane(id);
          }}
        />
      ) : (
        <span className={cn("min-w-0 truncate", focused ? "text-foreground" : "text-muted-foreground")}>{title}</span>
      )}
      {look && (
        <Tip label={lookLabel(look, pane.agent?.name)}>
          <StatusDot look={look} />
        </Tip>
      )}
      <div className={cn("ml-auto flex shrink-0 items-center", !focused && "opacity-0 group-hover/pane:opacity-100")}>
        {action("Split right", Columns2, () => void splitActive("row", id))}
        {action("Split down", Rows2, () => void splitActive("col", id))}
        {action("Kill pane", X, () => void killPane(id))}
      </div>
    </div>
  );
}
