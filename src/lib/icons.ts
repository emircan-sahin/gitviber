// File and folder icons from Material Icon Theme (MIT), resolved the way VS Code does.
import {
  file as defaultFile,
  folder as defaultFolder,
  folderExpanded as defaultFolderOpen,
  fileExtensions,
  fileNames,
  folderNames,
  folderNamesExpanded,
  iconDefinitions,
  languageIds,
} from "material-icon-theme/dist/material-icons.json";
import { languageFor } from "./language";

const byFileName = fileNames as Record<string, string>;
const byExtension = fileExtensions as Record<string, string>;
const byLanguage = languageIds as Record<string, string>;
const byFolder = folderNames as Record<string, string>;
const byFolderOpen = folderNamesExpanded as Record<string, string>;
const definitions = iconDefinitions as Record<string, { iconPath: string }>;

// Each SVG is its own asset; the browser fetches only icons that appear on screen.
const urls = import.meta.glob<string>("/node_modules/material-icon-theme/icons/*.svg", { query: "?url", import: "default", eager: true });

function urlFor(id: string | undefined) {
  const def = id && definitions[id];
  if (!def) return undefined;
  return urls[`/node_modules/material-icon-theme/icons/${def.iconPath.split("/").pop()}`];
}

const fileCache = new Map<string, string>();

export function fileIconUrl(path: string): string {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const lower = name.toLowerCase();
  // Keyed by the whole path: the language fallback depends on it (`.ssh/config`, `.git/config`).
  const hit = fileCache.get(path);
  if (hit) return hit;
  let id = byFileName[name] ?? byFileName[lower];
  // Longest compound extension first: "vault.controller.ts" → "controller.ts" → "ts".
  for (let i = lower.indexOf("."); !id && i !== -1; i = lower.indexOf(".", i + 1)) {
    id = byExtension[lower.slice(i + 1)];
  }
  id ??= byLanguage[languageFor(path)];
  const url = urlFor(id) ?? urlFor(defaultFile)!;
  fileCache.set(path, url);
  return url;
}

export function folderIconUrl(name: string, open: boolean): string {
  const lower = name.toLowerCase();
  const id = open ? (byFolderOpen[lower] ?? defaultFolderOpen) : (byFolder[lower] ?? defaultFolder);
  return urlFor(id) ?? urlFor(open ? defaultFolderOpen : defaultFolder)!;
}
