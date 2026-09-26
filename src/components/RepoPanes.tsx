import { ChevronRight } from "lucide-react";
import { type ReactNode, useState } from "react";
import { useDefaultLayout } from "react-resizable-panels";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { cn } from "@/lib/utils";
import { readJson, stringList, writeJson } from "@/lib/storage";
import { CountBadge } from "./CountBadge";

export interface Pane {
  id: string;
  title: string;
  /** The repository, owner/name. */
  detail: string;
  actions?: ReactNode;
  /** Always shown, unlike `actions`: a count, say. */
  badge?: ReactNode;
  children: ReactNode;
  /** The content scrolls itself (the history list does); the pane only sizes it. */
  scrolls?: boolean;
}

/**
 * VS Code-style stacked panes, for a fork's own and original lists. Each opens and closes on
 * its own; open ones share the height and resize against each other, and with all closed the
 * headers stack at the top. Collapsed state and sizes are remembered per `id`.
 *
 * Open panes resize as one group, so a closed pane between two open ones would move out of
 * place: fine for the two panes this is used with.
 */
export function RepoPanes({ id, panes }: { id: string; panes: Pane[] }) {
  const key = `gitviber.${id}-panes.collapsed`;
  const [collapsed, setCollapsed] = useState(() => stringList(readJson<unknown>(key, [])));
  const toggle = (pane: string) => {
    const next = collapsed.includes(pane) ? collapsed.filter((p) => p !== pane) : [...collapsed, pane];
    setCollapsed(next);
    // Remembered for this session only when storage fails.
    writeJson(key, next);
  };
  const open = panes.filter((p) => !collapsed.includes(p.id));
  // The library balances whatever it's given; closed panes stay out of it.
  const layout = useDefaultLayout({ id: `gitviber-${id}-panes-v2`, storage: localStorage, panelIds: open.map((p) => p.id) });
  const first = panes.findIndex((p) => !collapsed.includes(p.id));
  const closed = (p: Pane) => collapsed.includes(p.id);

  const header = (p: Pane) => <PaneHeader key={`${p.id}:header`} pane={p} open={!closed(p)} onToggle={() => toggle(p.id)} />;
  // One keyed list, and open panes always in the one group: opening or closing a pane leaves
  // the others mounted, so an expanded commit or a scroll position survives it.
  return (
    <div className="flex h-full flex-col">
      {[
        ...panes.slice(0, first < 0 ? panes.length : first).map(header),
        open.length > 0 && (
          <div key="open" className="min-h-0 flex-1">
            <ResizablePanelGroup orientation="vertical" defaultLayout={layout.defaultLayout} onLayoutChanged={layout.onLayoutChanged}>
              {open.map((p, i) => [
                i > 0 && (
                  <ResizableHandle
                    key={`${p.id}:handle`}
                    className="h-px w-full bg-border after:inset-x-0 after:inset-y-auto after:top-1/2 after:left-0 after:h-2 after:w-full after:translate-x-0 after:-translate-y-1/2"
                  />
                ),
                <ResizablePanel key={p.id} id={p.id} minSize={84}>
                  <div className="flex h-full flex-col">
                    {header(p)}
                    <div className={cn("min-h-0 flex-1", !p.scrolls && "overflow-x-hidden overflow-y-auto py-1")}>{p.children}</div>
                  </div>
                </ResizablePanel>,
              ])}
            </ResizablePanelGroup>
          </div>
        ),
        ...(first < 0 ? [] : panes.slice(first).filter(closed).map(header)),
      ]}
    </div>
  );
}

function PaneHeader({ pane, open, onToggle }: { pane: Pane; open: boolean; onToggle: () => void }) {
  return (
    <div className="group flex h-6 shrink-0 items-center border-b border-border pr-1">
      <button onClick={onToggle} aria-expanded={open} className="flex h-full min-w-0 flex-1 items-center gap-1 pl-1 text-left">
        <ChevronRight className={cn("size-3.5 shrink-0 text-subtle transition-transform", open && "rotate-90")} />
        <span className="shrink-0 text-[10.5px] font-semibold tracking-[0.08em] uppercase">{pane.title}</span>
        {/* Beside the title (past a long detail it went unseen), badged as the tabs' counts are: an open pane's as the active tab's. */}
        {pane.badge !== undefined && <CountBadge active={open}>{pane.badge}</CountBadge>}
        <span className="ml-1 min-w-0 truncate font-mono text-[10.5px] text-subtle">{pane.detail}</span>
      </button>
      {/* Like VS Code's pane actions: out of the way until the header is hovered. */}
      {pane.actions && <div className="flex shrink-0 items-center opacity-0 group-focus-within:opacity-100 group-hover:opacity-100">{pane.actions}</div>}
    </div>
  );
}
