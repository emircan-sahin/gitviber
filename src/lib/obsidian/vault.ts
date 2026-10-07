import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { type Vault, vaultApi } from "../api";
import { claimEdits, openStore } from "../editor/edits";
import { basename } from "../path";
import { editPath, vaultEditFile } from "../repo/selection";
import { useSettings } from "../settings";
import { readJson, writeJson } from "../storage";
import { createStore } from "../store";
import { refreshVaults, vaultList } from "./vaultList";

// Unsaved edits of their notes come back as the list is read, with their tabs' dots.
vaultList.subscribe(() => vaultList.get()?.forEach((v) => openVaultEdits(v.path)));

const PICKED_KEY = "gitviber.obsidian.vault";
const picked = createStore<string | null>(readJson<string | null>(PICKED_KEY, null, (v): v is string => typeof v === "string"));

/** Shows `vault` (its path) in the explorer from now on. */
export function pickVault(vault: string) {
  picked.set(vault);
  writeJson(PICKED_KEY, vault);
}

/**
 * The vaults the explorer may show (Obsidian's, less those turned off in Settings) and the one
 * it shows: the one picked last, else the last opened in Obsidian. None while the feature is off.
 */
export function useVaults(): { vaults: Vault[]; vault: Vault | null } {
  const { obsidian, hiddenVaults } = useSettings();
  const all = vaultList.use();
  const want = picked.use();
  useEffect(() => {
    if (obsidian && !all) void refreshVaults();
  }, [obsidian, all]);
  const vaults = obsidian ? (all ?? []).filter((v) => !hiddenVaults.includes(v.path)) : [];
  return { vaults, vault: vaults.find((v) => v.path === want) ?? vaults[0] ?? null };
}

/**
 * How current what's read from each vault is: `text` moves on any change (notes read again),
 * `files` when files came, went or moved (the file list for links), and each file has its own
 * count (media read again only when it changed), started over under a new `all` when events
 * were dropped and any file may have.
 */
interface Revisions {
  text: number;
  files: number;
  all: number;
  file: Record<string, number>;
}
const revisions = createStore<Record<string, Revisions>>({});
const NONE: Revisions = { text: 0, files: 0, all: 0, file: {} };

/** `paths` changed in `vault`; none named: anything may have. */
function bump(vault: string, files: boolean, paths: string[]) {
  const now = revisions.get();
  const was = now[vault] ?? NONE;
  const file = paths.length ? { ...was.file, ...Object.fromEntries(paths.map((p) => [p, (was.file[p] ?? 0) + 1])) } : {};
  revisions.set({ ...now, [vault]: { text: was.text + 1, files: was.files + (files || !paths.length ? 1 : 0), all: was.all + (paths.length ? 0 : 1), file } });
}

let watched: string | null = null;

/**
 * Follows the vault the explorer shows, and while there is one, the others too: they're read
 * again when the window comes back into focus, as no watcher covers them. Used by the explorer
 * section, as the repo's watcher is by the workspace.
 */
export function useVaultEvents(vault: string | null) {
  useEffect(() => {
    if (!vault) return void vaultApi.watch(null).catch(() => {});
    watched = vault;
    vaultApi.watch(vault).catch(() => (watched = null));
    const changed = listen<{ vault: string; files: boolean; paths: string[] }>("vault-changed", ({ payload: p }) => bump(p.vault, p.files, p.paths));
    const focus = () => {
      for (const v of vaultList.get() ?? []) if (v.path !== watched) bump(v.path, true, []);
      void refreshVaults();
    };
    window.addEventListener("focus", focus);
    return () => {
      watched = null;
      window.removeEventListener("focus", focus);
      void changed.then((stop) => stop());
    };
  }, [vault]);
}

const useRevisions = (vault: string) => revisions.use()[vault] ?? NONE;

/** Changes whenever notes in `vault` may have changed. */
export const useVaultRevision = (vault: string) => useRevisions(vault).text;

/** Changes when files in `vault` came, went or moved: what its folders list. */
export const useVaultFilesRevision = (vault: string) => useRevisions(vault).files;

/**
 * Changes when `path` in `vault` changed, was renamed or came back (or anything may have): not
 * when another file comes or goes, which an open video or PDF shouldn't reload for.
 */
export function useFileRevision(vault: string, path: string) {
  const r = useRevisions(vault);
  return `${r.all}.${r.file[path] ?? 0}`;
}

// Each vault's file list as of the `files` revision it was read at; links resolve against it.
const fileLists = new Map<string, { rev: number; files: Promise<string[]> }>();

export function vaultFiles(vault: string): Promise<string[]> {
  const rev = (revisions.get()[vault] ?? NONE).files;
  const hit = fileLists.get(vault);
  if (hit?.rev === rev) return hit.files;
  const files = vaultApi.files(vault);
  fileLists.set(vault, { rev, files });
  files.catch(() => fileLists.delete(vault));
  return files;
}

/** Every file in `vault`, kept current; null until first read. */
export function useVaultFiles(vault: string): string[] | null {
  const rev = useRevisions(vault).files;
  const [files, setFiles] = useState<{ vault: string; list: string[] } | null>(null);
  useEffect(() => {
    let live = true;
    vaultFiles(vault).then(
      (list) => live && setFiles({ vault, list }),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [vault, rev]);
  return files?.vault === vault ? files.list : null;
}

/** The store a vault's unsaved edits are kept in (lib/editor/edits), apart from any repo's. */
export function openVaultEdits(vault: string) {
  const path = (key: string) => vaultEditFile(vault, key);
  openStore(`obsidian:${vault}`, {
    name: (key) => basename(path(key)),
    read: (key) => vaultApi.readFile(vault, path(key)),
    // Read again after a save: a vault no watcher covers wouldn't be, and the view would show the old text.
    write: (key, text) => vaultApi.writeFile(vault, path(key), text).then(() => bump(vault, false, [path(key)])),
  });
}

/** Edits to this vault file are kept with its vault. */
export function claimVaultEdits(vault: string, path: string) {
  claimEdits(editPath({ kind: "vault", vault, path })!, `obsidian:${vault}`);
}
