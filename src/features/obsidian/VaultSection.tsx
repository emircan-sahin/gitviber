import { ChevronDown, ChevronRight, ChevronsDownUp, Copy, ExternalLink, File, FolderSearch } from "lucide-react";
import { type ReactNode, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { useDefaultLayout } from "react-resizable-panels";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "@/components/ui/context-menu";
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Tip } from "@/components/ui/tooltip";
import { FileIcon, FolderIcon } from "@/components/FileIcon";
import { Row } from "@/features/explorer/FileTree";
import { treeKey, treeRows, useLazyTree } from "@/features/explorer/lazyTree";
import { errorMessage, type Vault, vaultApi, type VaultEntry } from "@/lib/api";
import { copyText } from "@/lib/app/clipboard";
import { failed, toast } from "@/lib/app/toast";
import { noteName } from "@/lib/obsidian/links";
import { pickVault, useVaultEvents, useVaultFilesRevision, useVaults } from "@/lib/obsidian/vault";
import { REVEAL_FAILED, REVEAL_LABEL } from "@/lib/platform";
import { type Selection, selectionKey } from "@/lib/repo/selection";
import { readJson, writeJson } from "@/lib/storage";
import { createStore } from "@/lib/store";
import { focusPanel } from "@/lib/ui/panels";
import { isMenuKey, openRowMenu } from "@/lib/ui/useListNav";
import { cn } from "@/lib/utils";

const COLLAPSED_KEY = "gitviber.obsidian.collapsed";
const collapsed = createStore(readJson<boolean>(COLLAPSED_KEY, false, (v): v is boolean => typeof v === "boolean"));
const toggleCollapsed = () => {
  collapsed.set(!collapsed.get());
  writeJson(COLLAPSED_KEY, collapsed.get());
};

// Open folders per vault, kept for the app's life: the tree remounts when its section closes.
const openFolders = new Map<string, Set<string>>();

/**
 * The explorer's files, with the Obsidian vault under them when Obsidian lists one: a section
 * that opens and closes like Changes' Staged, resizing against the files while open and a
 * header at the bottom while closed.
 */
export function ExplorerPanes({ tree, activeKey, onOpen }: { tree: ReactNode; activeKey: string | null; onOpen: (s: Selection, pin?: boolean) => void }) {
  const { vaults, vault } = useVaults();
  const closed = collapsed.use();
  const open = !!vault && !closed;
  const layout = useDefaultLayout({ id: "gitviber-explorer-vault-v1", storage: localStorage, panelIds: open ? ["tree", "vault"] : ["tree"] });
  const treeRef = useRef<VaultTreeHandle>(null);
  // The vault on show is watched, so its tree and open notes follow edits made in Obsidian.
  useVaultEvents(vault?.path ?? null);

  const header = vault && <VaultHeader vaults={vaults} vault={vault} open={open} onCollapseAll={() => treeRef.current?.collapseAll()} />;
  return (
    <div className="flex h-full flex-col">
      <div className="min-h-0 flex-1">
        {/* One group, the files always its first panel: opening the vault leaves them mounted. */}
        <ResizablePanelGroup orientation="vertical" defaultLayout={layout.defaultLayout} onLayoutChanged={layout.onLayoutChanged}>
          <ResizablePanel id="tree" minSize={80}>
            {tree}
          </ResizablePanel>
          {open && (
            <>
              <ResizableHandle className="bg-border" />
              <ResizablePanel id="vault" minSize={84} defaultSize="40">
                <div className="flex h-full flex-col">
                  {header}
                  <VaultTree key={vault.path} ref={treeRef} vault={vault} activeKey={activeKey} onOpen={onOpen} />
                </div>
              </ResizablePanel>
            </>
          )}
        </ResizablePanelGroup>
      </div>
      {!open && header}
    </div>
  );
}

function VaultHeader({ vaults, vault, open, onCollapseAll }: { vaults: Vault[]; vault: Vault; open: boolean; onCollapseAll: () => void }) {
  return (
    <div className={cn("group flex h-7 shrink-0 items-center gap-1 border-border bg-panel pr-1 pl-2", open ? "border-b" : "border-t")}>
      <button
        aria-expanded={open}
        onClick={toggleCollapsed}
        className="flex shrink-0 items-center gap-1 text-[10.5px] font-semibold tracking-[0.08em] text-subtle uppercase hover:text-foreground focus-visible:text-foreground"
      >
        <ChevronDown className={cn("size-3 transition-transform", !open && "-rotate-90")} />
        Obsidian
      </button>
      {vaults.length > 1 ? (
        <DropdownMenu>
          <Tip label="Switch vault">
            <DropdownMenuTrigger asChild>
              <button className="flex min-w-0 items-center gap-0.5 rounded-sm px-1 font-mono text-[10.5px] text-subtle hover:bg-hover hover:text-foreground focus-visible:bg-hover focus-visible:text-foreground">
                <span className="truncate">{vault.name}</span>
                <ChevronDown className="size-3 shrink-0" />
              </button>
            </DropdownMenuTrigger>
          </Tip>
          <DropdownMenuContent align="start" className="min-w-44">
            <DropdownMenuRadioGroup value={vault.path} onValueChange={pickVault}>
              {vaults.map((v) => (
                <DropdownMenuRadioItem key={v.path} value={v.path}>
                  <span className="truncate" title={v.path}>
                    {v.name}
                  </span>
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      ) : (
        <span className="min-w-0 truncate px-1 font-mono text-[10.5px] text-subtle" title={vault.path}>
          {vault.name}
        </span>
      )}
      {open && (
        <div className="ml-auto flex shrink-0 items-center opacity-0 group-focus-within:opacity-100 group-hover:opacity-100">
          <Tip label="Collapse folders">
            <button aria-label="Collapse folders" onClick={onCollapseAll} className="flex size-6 items-center justify-center rounded-sm text-subtle hover:bg-hover hover:text-foreground focus-visible:bg-hover focus-visible:text-foreground">
              <ChevronsDownUp className="size-3.5" />
            </button>
          </Tip>
        </div>
      )}
    </div>
  );
}

interface VaultTreeHandle {
  collapseAll: () => void;
}

/** The vault's folders and files as Obsidian's file explorer lists them, opened lazily like the explorer's. */
function VaultTree({ vault, activeKey, onOpen, ref }: { vault: Vault; activeKey: string | null; onOpen: (s: Selection, pin?: boolean) => void; ref: React.Ref<VaultTreeHandle> }) {
  const { children, expanded, setExpanded, setOpen } = useLazyTree<VaultEntry>({
    list: (path) => vaultApi.listDir(vault.path, path),
    // Not on every note's autosave: folders list again only when files came, went or moved.
    revision: useVaultFilesRevision(vault.path),
    onRootError: (e) => toast("error", `Could not list ${vault.name}`, errorMessage(e)),
    initial: openFolders.get(vault.path),
  });
  useEffect(() => void openFolders.set(vault.path, expanded), [vault.path, expanded]);
  const [selected, setSelected] = useState<string | null>(null);
  const [menu, setMenu] = useState<VaultEntry | null>(null);
  const treeRef = useRef<HTMLDivElement>(null);
  useImperativeHandle(ref, () => ({ collapseAll: () => setExpanded(new Set([""])) }), [setExpanded]);

  const rows = useMemo(() => treeRows(children, expanded), [children, expanded]);
  const sel = (path: string): Selection => ({ kind: "vault", vault: vault.path, path });
  const activate = (e: VaultEntry, pin = false) => (e.isDir ? setOpen(e.path, !expanded.has(e.path)) : onOpen(sel(e.path), pin));
  const rowOf = (path: string) => treeRef.current?.querySelector(`[data-path="${CSS.escape(path)}"]`);

  const onKeyDown = (ev: React.KeyboardEvent) => {
    if (ev.target !== ev.currentTarget) return;
    if (isMenuKey(ev)) {
      const row = selected ? rowOf(selected) : null;
      openRowMenu(row instanceof HTMLElement ? row : (ev.currentTarget as HTMLElement));
      return ev.preventDefault();
    }
    // ⌥ and ⌘ arrows belong to the global shortcuts.
    if (ev.altKey || ev.metaKey) return;
    const i = rows.findIndex((r) => r.entry.path === selected);
    const k = treeKey(ev.key, rows, i, { isOpen: (p) => expanded.has(p), row: rows.length ? rowOf(rows[Math.max(i, 0)].entry.path) : null });
    if (!k) return;
    ev.preventDefault();
    if ("move" in k) {
      const row = rows[Math.max(0, Math.min(rows.length - 1, k.move))];
      if (row) setSelected(row.entry.path);
    } else if ("open" in k) setOpen(k.open, true);
    else if ("close" in k) setOpen(k.close, false);
    else if ("select" in k) setSelected(k.select);
    else {
      activate(k.activate, k.pin);
      if (k.focusCode) focusPanel("code");
    }
  };
  // The keyboard's row stays in view.
  useEffect(() => {
    if (selected) rowOf(selected)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          ref={treeRef}
          tabIndex={0}
          role="tree"
          aria-label={`${vault.name} vault`}
          onKeyDown={onKeyDown}
          onContextMenu={(ev) => {
            const path = (ev.target as HTMLElement).closest<HTMLElement>("[data-path]")?.dataset.path;
            const entry = rows.find((r) => r.entry.path === path)?.entry ?? null;
            setMenu(entry);
            if (entry) setSelected(entry.path);
          }}
          className="group/tree min-h-0 flex-1 overflow-x-hidden overflow-y-auto py-1 outline-none"
        >
          {children[""]?.length === 0 && <div className="px-4 py-6 text-center text-[12px] text-subtle">This vault is empty.</div>}
          {rows.map(({ entry: e, depth }) => {
            const isOpen = expanded.has(e.path);
            const active = !e.isDir && activeKey === selectionKey(sel(e.path));
            return (
              <Row
                key={e.path}
                depth={depth}
                path={e.path}
                aria-level={depth + 1}
                aria-expanded={e.isDir ? isOpen : undefined}
                aria-selected={active}
                onClick={() => {
                  setSelected(e.path);
                  activate(e);
                }}
                onDoubleClick={() => !e.isDir && onOpen(sel(e.path), true)}
                className={cn(
                  active ? "bg-primary/15" : "hover:bg-hover",
                  selected === e.path && "group-focus/tree:bg-hover group-focus/tree:outline group-focus/tree:-outline-offset-1 group-focus/tree:outline-primary/70",
                )}
              >
                {active && <span className="absolute inset-y-0 left-0 w-0.5 bg-primary" />}
                {e.isDir ? (
                  <>
                    <ChevronRight className={cn("size-3 shrink-0 text-subtle transition-transform duration-100", isOpen && "rotate-90")} />
                    <FolderIcon name={e.name} open={isOpen} />
                  </>
                ) : (
                  <>
                    <span className="w-3 shrink-0" />
                    <FileIcon path={e.path} />
                  </>
                )}
                {/* Notes by their name, as Obsidian lists them. */}
                <span className="truncate text-foreground/85">{e.isDir ? e.name : noteName(e.path)}</span>
              </Row>
            );
          })}
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent>
        {menu && !menu.isDir && (
          <>
            <ContextMenuItem onSelect={() => onOpen(sel(menu.path), true)}>
              <File /> Open
            </ContextMenuItem>
            <ContextMenuItem onSelect={() => vaultApi.openInObsidian(vault.path, menu.path).catch(failed("Could not open Obsidian"))}>
              <ExternalLink /> Open in Obsidian
            </ContextMenuItem>
            <ContextMenuSeparator />
          </>
        )}
        <ContextMenuItem onSelect={() => vaultApi.reveal(vault.path, menu?.path ?? "").catch(failed(REVEAL_FAILED))}>
          <FolderSearch /> {REVEAL_LABEL}
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => copyText(menu ? `${vault.path}/${menu.path}` : vault.path, "Path copied")}>
          <Copy /> Copy Path
        </ContextMenuItem>
        {menu && (
          <ContextMenuItem onSelect={() => copyText(menu.path, "Relative path copied")}>
            <Copy /> Copy Relative Path
          </ContextMenuItem>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}
