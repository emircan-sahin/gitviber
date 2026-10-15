import { invoke } from "@tauri-apps/api/core";
import type { FileText } from "./types";

/** A file or folder outside the repo, by its real path. */
export interface ExternalFound {
  path: string;
  kind: "file" | "dir";
}

/** Files outside the repo that terminal output names (external.rs): read, never written. */
export const externalApi = {
  /** Each absolute or `~` path as it is on disk, null when there's neither a file nor a folder. */
  find: (paths: string[]) => invoke<(ExternalFound | null)[]>("external_find", { paths }),
  readFile: (path: string) => invoke<FileText>("external_read_file", { path }),
  media: (path: string) => invoke<ArrayBuffer>("external_media", { path }),
  reveal: (path: string) => invoke<void>("external_reveal", { path }),
};
