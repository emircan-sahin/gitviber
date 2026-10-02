import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { type Vault, vaultApi } from "../api";
import { useSettings } from "../settings";
import { readJson, writeJson } from "../storage";
import { createStore } from "../store";

/** Obsidian's vaults, as last read (null before the first read). */
export const vaultList = createStore<Vault[] | null>(null);

let reading: Promise<Vault[]> | null = null;
/** Reads Obsidian's vault list again: a vault opened, added or removed in Obsidian shows up. */
export function refreshVaults(): Promise<Vault[]> {
  reading ??= vaultApi
    .vaults()
    .catch(() => [])
    .then((vaults) => {
      const was = vaultList.get();
      if (!was || JSON.stringify(was) !== JSON.stringify(vaults)) vaultList.set(vaults);
      return vaults;
    })
    .finally(() => (reading = null));
  return reading;
}

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

// Each vault's revision: bumped when its watcher sees a change, or for one no watcher covers,
// when the window comes back into focus (it may have changed meanwhile).
const revisions = createStore<Record<string, number>>({});
const bump = (vaults: string[]) => {
  const now = revisions.get();
  revisions.set({ ...now, ...Object.fromEntries(vaults.map((v) => [v, (now[v] ?? 0) + 1])) });
};
listen<{ vault: string }>("vault-changed", (e) => bump([e.payload.vault])).catch(() => {});
window.addEventListener("focus", () => {
  bump((vaultList.get() ?? []).map((v) => v.path).filter((v) => v !== watched));
  void refreshVaults();
});

let watched: string | null = null;
/** Watches `vault` for changes (one at a time; null: none). */
export function watchVault(vault: string | null) {
  watched = vault;
  vaultApi.watch(vault).catch(() => (watched = null));
}

/** Changes whenever files in `vault` may have changed. */
export const useVaultRevision = (vault: string | null) => {
  const all = revisions.use();
  return vault ? (all[vault] ?? 0) : 0;
};

// Each vault's file list at the revision it was read at; links resolve against it.
const fileLists = new Map<string, { rev: number; files: Promise<string[]> }>();

export function vaultFiles(vault: string): Promise<string[]> {
  const rev = revisions.get()[vault] ?? 0;
  const hit = fileLists.get(vault);
  if (hit?.rev === rev) return hit.files;
  const files = vaultApi.files(vault);
  fileLists.set(vault, { rev, files });
  files.catch(() => fileLists.delete(vault));
  return files;
}

/** Every file in `vault`, kept current; null until first read. */
export function useVaultFiles(vault: string): string[] | null {
  const rev = useVaultRevision(vault);
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

/** A vault file's full path (see editPath) back to its vault and its path in there; null when it's in none. */
export function splitVaultPath(full: string, vaults: Vault[]): { vault: string; path: string } | null {
  const vault = vaults.filter((v) => full.startsWith(`${v.path}/`)).sort((a, b) => b.path.length - a.path.length)[0];
  return vault ? { vault: vault.path, path: full.slice(vault.path.length + 1) } : null;
}

/** Reads and writes a vault file named by its full path, for saving edits made in the code view. */
export async function vaultFileIO(full: string) {
  const at = splitVaultPath(full, vaultList.get() ?? (await refreshVaults()));
  if (!at) throw new Error(`${full} is in none of Obsidian's vaults`);
  // The vault reads again after a save: a vault no watcher covers wouldn't, and the view would go back to the old text.
  return { read: () => vaultApi.readFile(at.vault, at.path), write: (text: string) => vaultApi.writeFile(at.vault, at.path, text).then(() => bump([at.vault])) };
}
