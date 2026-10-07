import { type Vault, vaultApi } from "../api";
import { createStore } from "../store";

// Apart from vault.ts, which opens the vaults' unsaved edits: Settings reads the list too, and the
// settings window keeps no edits (its copy would overwrite the workspace's as it closes).

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
