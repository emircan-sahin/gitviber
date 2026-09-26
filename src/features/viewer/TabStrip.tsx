import { History, X } from "lucide-react";
import { type RefObject, useLayoutEffect, useRef } from "react";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuShortcut, ContextMenuTrigger } from "@/components/ui/context-menu";
import { type Selection, selectionPath } from "@/lib/repo/selection";
import { matchesCommand, useShortcut } from "@/lib/commands/keybindings";
import { isMenuKey, openRowMenu } from "@/lib/ui/useListNav";
import { cn } from "@/lib/utils";
import { useEdited } from "@/lib/editor/edits";
import { basename } from "@/lib/path";
import { SortableList, useSortableItem } from "@/components/Sortable";
import { IssueStateIcon, PullStateIcon } from "@/features/github/shared/StateBadges";
import { FileIcon } from "@/components/FileIcon";
import { type Tab, type TabGroup, tabGroup } from "./tabs";

interface Props {
  tabs: Tab[];
  active: Tab | null;
  onActivate: (key: string) => void;
  onClose: (key: string) => void;
  onCloseTabs: (keys: string[]) => void;
  onPin: (key: string) => void;
  onMoveTab: (from: number, to: number) => void;
  onShowHistory: (path: string) => void;
}

function tabLabel(sel: Selection) {
  if (sel.kind === "pull" || sel.kind === "issue") return selectionPath(sel);
  return basename(selectionPath(sel));
}

/**
 * A tablist: the open tab is its one tab stop; ←/→ (Home/End) switch tabs, ↵ or Space keeps a
 * preview tab, ⌫ closes, ⌥←/⌥→ reorder (tab.moveLeft / tab.moveRight), ⇧F10 opens the tab's menu.
 */
export function TabStrip({ tabs, active, onActivate, onClose, onCloseTabs, onPin, onMoveTab, onShowHistory }: Props) {
  const strip = useRef<HTMLDivElement>(null);
  // Set when a tab holding focus closes: focus goes on to the tab that opens in its place.
  const lostFocus = useRef(false);
  useLayoutEffect(() => {
    if (!lostFocus.current) return;
    lostFocus.current = false;
    strip.current?.querySelector<HTMLElement>('[role="tab"][tabindex="0"]')?.focus();
  });

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const el = e.target instanceof HTMLElement && e.target.getAttribute("role") === "tab" ? e.target : null;
    if (!el) return;
    const els = [...e.currentTarget.querySelectorAll<HTMLElement>('[role="tab"]')];
    const i = els.indexOf(el);
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    const shift = matchesCommand("tab.moveRight", e.nativeEvent) ? 1 : matchesCommand("tab.moveLeft", e.nativeEvent) ? -1 : 0;
    if (!shift && (e.metaKey || e.ctrlKey)) return;
    if (shift) {
      if (!tabs[i + shift]) return;
      onMoveTab(i, i + shift);
      // React may move this very node, and a node taken out of the page loses focus.
      requestAnimationFrame(() => {
        el.focus();
        el.scrollIntoView({ block: "nearest", inline: "nearest" });
      });
    } else if (isMenuKey(e)) openRowMenu(el);
    else if (e.shiftKey || e.altKey) return;
    else if (step || e.key === "Home" || e.key === "End") {
      const to = e.key === "Home" ? 0 : e.key === "End" ? els.length - 1 : Math.max(0, Math.min(els.length - 1, i + step));
      onActivate(tabs[to].key);
      els[to].focus();
      els[to].scrollIntoView({ block: "nearest", inline: "nearest" });
    } else if (e.key === "Enter" || e.key === " ") onPin(tabs[i].key);
    else if (e.key === "Backspace" || e.key === "Delete") onClose(tabs[i].key);
    else return;
    e.preventDefault();
  };

  return (
    <div
      ref={strip}
      role="tablist"
      aria-label="Open tabs"
      onKeyDown={onKeyDown}
      data-tauri-drag-region
      data-scrollbar="none"
      className="flex h-9 shrink-0 items-stretch overflow-x-auto overflow-y-hidden border-b border-border bg-panel"
    >
      <SortableList ids={tabs.map((t) => t.key)} axis="x" onMove={onMoveTab}>
        {tabs.map((t, i) => (
          <TabItem
            key={t.key}
            tab={t}
            active={t.key === active?.key}
            tabStop={active ? t.key === active.key : i === 0}
            lostFocus={lostFocus}
            onActivate={onActivate}
            onClose={onClose}
            closes={(which) => tabGroup(tabs, i, which).length > 0}
            onCloseGroup={(which) => onCloseTabs(tabGroup(tabs, i, which))}
            onPin={onPin}
            onShowHistory={onShowHistory}
          />
        ))}
      </SortableList>
    </div>
  );
}

function TabItem({
  tab: t,
  active: isActive,
  tabStop,
  lostFocus,
  onActivate,
  onClose,
  closes,
  onCloseGroup,
  onPin,
  onShowHistory,
}: {
  tab: Tab;
  active: boolean;
  tabStop: boolean;
  lostFocus: RefObject<boolean>;
  onActivate: (key: string) => void;
  onClose: (key: string) => void;
  /** Whether Close Others (…) around this tab would close any. */
  closes: (which: TabGroup) => boolean;
  onCloseGroup: (which: TabGroup) => void;
  onPin: (key: string) => void;
  onShowHistory: (path: string) => void;
}) {
  const { props, dragging, guard } = useSortableItem(t.key);
  // Unsaved edits: a dot where the close button goes, the button on hover (as VS Code).
  const unsaved = useEdited().has(selectionPath(t.sel)) && t.sel.kind === "file";
  const closeKey = useShortcut("tab.close");
  const closeOthersKey = useShortcut("tab.closeOthers");
  const el = useRef<HTMLDivElement | null>(null);
  // Runs before the node leaves the page, while it can still say whether it had focus.
  useLayoutEffect(
    () => () => {
      if (el.current?.contains(document.activeElement)) lostFocus.current = true;
    },
    [lostFocus],
  );
  const tab = (
    <div
      {...props}
      ref={(node) => {
        props.ref(node);
        el.current = node;
      }}
      role="tab"
      aria-selected={isActive}
      tabIndex={tabStop ? 0 : -1}
      onClick={guard(() => onActivate(t.key))}
      onDoubleClick={() => onPin(t.key)}
      onAuxClick={(e) => e.button === 1 && onClose(t.key)}
      className={cn(
        "group relative flex max-w-56 shrink-0 cursor-pointer items-center gap-1.5 border-r border-border pr-1.5 pl-3 text-[12px] outline-none select-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset",
        isActive ? "bg-background text-foreground" : "bg-panel text-muted-foreground hover:bg-hover hover:text-foreground focus:bg-hover focus:text-foreground",
        dragging && "cursor-grabbing bg-elevated text-foreground shadow-lg ring-1 shadow-black/50 ring-border-strong",
      )}
    >
      {isActive && <span className="absolute inset-x-0 top-0 h-px bg-primary" />}
      {isActive && !dragging && <span className="absolute inset-x-0 -bottom-px h-px bg-background" />}
      {t.sel.kind === "pull" ? (
        <PullStateIcon pull={t.sel.pull} />
      ) : t.sel.kind === "issue" ? (
        <IssueStateIcon issue={t.sel.issue} />
      ) : (
        <FileIcon path={selectionPath(t.sel)} />
      )}
      <span className={cn("truncate", t.preview && "italic")}>{tabLabel(t.sel)}</span>
      <TabKind sel={t.sel} />
      <button
        aria-label={unsaved ? "Close tab (unsaved changes)" : "Close tab"}
        // Off the Tab order: the tab closes with ⌫, and one stop per tab would crowd it.
        tabIndex={-1}
        // Pressing the close button must not start a drag.
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.stopPropagation();
          onClose(t.key);
        }}
        className={cn(
          "flex size-5 items-center justify-center rounded-sm text-subtle hover:bg-active focus-visible:bg-active hover:text-foreground focus-visible:text-foreground",
          !isActive && !unsaved && "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100",
        )}
      >
        {unsaved && <span data-unsaved className="size-2 rounded-full bg-current group-hover:hidden" />}
        <X className={cn("size-3", unsaved && "hidden group-hover:block")} />
      </button>
    </div>
  );
  const file = t.sel.kind !== "pull" && t.sel.kind !== "issue";
  // As VS Code's tab menu, plus to the left.
  const group = (which: TabGroup, label: string, shortcut?: string) => (
    <ContextMenuItem disabled={!closes(which)} onSelect={() => onCloseGroup(which)}>
      {label}
      {shortcut && <ContextMenuShortcut>{shortcut}</ContextMenuShortcut>}
    </ContextMenuItem>
  );
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{tab}</ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={() => onClose(t.key)}>
          Close
          {isActive && closeKey && <ContextMenuShortcut>{closeKey}</ContextMenuShortcut>}
        </ContextMenuItem>
        {group("others", "Close Others", isActive ? closeOthersKey : undefined)}
        {group("left", "Close to the Left")}
        {group("right", "Close to the Right")}
        {group("all", "Close All")}
        {t.preview && (
          <>
            <ContextMenuSeparator />
            <ContextMenuItem onSelect={() => onPin(t.key)}>Keep Open</ContextMenuItem>
          </>
        )}
        {file && (
          <>
            <ContextMenuSeparator />
            <ContextMenuItem onSelect={() => onShowHistory(selectionPath(t.sel))}>
              <History /> Show History
            </ContextMenuItem>
          </>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}

function TabKind({ sel }: { sel: Selection }) {
  const labels: Partial<Record<Selection["kind"], string>> = { staged: "staged", unstaged: "diff", conflict: "conflict" };
  const label =
    sel.kind === "commit" ? sel.commit.shortSha : sel.kind === "pr-file" ? (sel.range.number ? `#${sel.range.number}` : (sel.range.label ?? "compare")) : sel.kind === "branch" ? `vs ${sel.label}` : labels[sel.kind];
  return label ? <span className="shrink-0 font-mono text-[10px] text-subtle">{label}</span> : null;
}
