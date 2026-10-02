import { useAsyncValue } from "@/hooks/useAsyncValue";
import { api, errorMessage } from "../api";
import { createStore } from "../store";
import { runCommand } from "../commands/keybindings";
import { toast } from "../app/toast";

type Tools = Awaited<ReturnType<typeof api.externalTools>>;

/** git's names for its built-in tools, as the apps call themselves. */
const NAMES: Record<string, string> = {
  araxis: "Araxis Merge",
  bc: "Beyond Compare",
  bc3: "Beyond Compare",
  bc4: "Beyond Compare",
  codecompare: "Code Compare",
  deltawalker: "DeltaWalker",
  diffmerge: "DiffMerge",
  gvimdiff: "gVim",
  kdiff3: "KDiff3",
  meld: "Meld",
  opendiff: "FileMerge",
  p4merge: "P4Merge",
  smerge: "Sublime Merge",
  tortoisemerge: "TortoiseMerge",
  vscode: "VS Code",
  winmerge: "WinMerge",
};
export const toolName = (tool: string) => NAMES[tool] ?? tool;

const NONE: Tools = { merge: null, diff: null };
// The last answer, shown at once while the config is read again (it's one git call).
let last: { root: string; tools: Tools } | null = null;

/** The tools `root`'s config sets up, read again on each mount: a menu opening, a conflict shown. */
export function useExternalTools(root: string): Tools {
  const initial = last?.root === root ? last.tools : NONE;
  return useAsyncValue(
    () =>
      api.externalTools().then((tools) => {
        last = { root, tools };
        return tools;
      }),
    [root],
    initial,
  );
}

/** git mergetool reads the path through a shell that loses a `"`, a `\` or a tab in it ("file not found"). */
export const toolCanOpen = (path: string) => !/["\\\t\n]/.test(path);

/** The paths open in the merge tool now: a second click waits for the first. */
const merging = createStore<ReadonlySet<string>>(new Set());
export const useMergingInTool = (path: string) => merging.use().has(path);
export const mergingInTool = (path: string) => merging.get().has(path);

/** Waits for the tool to be closed, then reads the repo again: git staged the file if it was merged. */
export async function mergeInTool(tool: string, path: string) {
  if (mergingInTool(path)) return;
  merging.set(new Set([...merging.get(), path]));
  try {
    await api.openMergeTool(path);
    toast("success", "Conflict resolved", `${path}, in ${toolName(tool)}`);
  } catch (e) {
    toast("info", `${path} is still in conflict`, errorMessage(e));
  } finally {
    merging.set(new Set([...merging.get()].filter((p) => p !== path)));
  }
  runCommand("repo.refresh");
}
/** Then reads the repo again too: a tool that edits (Meld, FileMerge) may have changed the file. */
export async function diffInTool(tool: string, path: string, staged: boolean) {
  try {
    await api.openDiffTool(path, staged);
  } catch (e) {
    toast("error", `Could not open ${toolName(tool)}`, errorMessage(e));
  }
  runCommand("repo.refresh");
}
