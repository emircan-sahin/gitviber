import { invoke } from "@tauri-apps/api/core";
import type { FileText } from "./types";

/** An Obsidian vault, from Obsidian's own list (obsidian.rs). `path` names it in every call. */
export interface Vault {
  id: string;
  name: string;
  path: string;
  open: boolean;
  /** Last opened, in ms since the epoch. */
  ts: number;
}

export interface VaultEntry {
  name: string;
  path: string;
  isDir: boolean;
}

export const vaultApi = {
  /** Every vault Obsidian lists whose folder is there, the last opened first. */
  vaults: () => invoke<Vault[]>("vaults"),
  listDir: (vault: string, path: string) => invoke<VaultEntry[]>("vault_list_dir", { vault, path }),
  /** Every file in the vault (vault-relative), for links to resolve against. */
  files: (vault: string) => invoke<string[]>("vault_files", { vault }),
  readFile: (vault: string, path: string) => invoke<FileText>("vault_read_file", { vault, path }),
  media: (vault: string, path: string) => invoke<ArrayBuffer>("vault_media", { vault, path }),
  writeFile: (vault: string, path: string, content: string) => invoke<void>("vault_write_file", { vault, path, content }),
  reveal: (vault: string, path: string) => invoke<void>("vault_reveal", { vault, path }),
  openInObsidian: (vault: string, path: string) => invoke<void>("vault_open_in_obsidian", { vault, path }),
  /** Watches `vault` for changes ("vault-changed" events); null stops. */
  watch: (vault: string | null) => invoke<void>("vault_watch", { vault }),
};
