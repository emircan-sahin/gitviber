import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { FileIcon } from "@/components/FileIcon";
import { LineCounts, PathLabel, StatusLetter } from "@/components/StatusBadge";
import { api, type PatchPreview } from "@/lib/api";
import { failed, toast } from "@/lib/app/toast";
import { rewriteFiles } from "@/lib/repo/undo";
import { createStore } from "@/lib/store";

/** The patch read from the clipboard, and what applying it would do, while the dialog asks. */
const shown = createStore<{ patch: string; preview: PatchPreview } | null>(null);

/** Reads a patch from the clipboard and asks before applying it (Apply Patch from Clipboard). */
export async function openApplyPatch() {
  try {
    const patch = await api.clipboardText();
    if (!patch.trim()) return toast("info", "The clipboard is empty", "Copy a patch first, such as one from Copy as Patch.");
    shown.set({ patch, preview: await api.patchPreview(patch) });
  } catch (e) {
    failed("Can't apply that patch")(e);
  }
}

const HOW = {
  clean: "It applies cleanly.",
  merge: "It applies only by merging with what changed here since, which may leave conflicts to resolve.",
};

/** The dialog, mounted once per window: its files, whether they apply, and Apply. The working tree only; ⌘Z undoes it. */
export function ApplyPatchDialog({ refresh }: { refresh: () => unknown }) {
  const open = shown.use();
  if (!open) return null;
  const { patch, preview } = open;
  const close = () => shown.set(null);
  const n = preview.files.length;
  const apply = () => {
    close();
    void rewriteFiles(n === 1 ? `Applied patch to ${preview.files[0].path}` : `Applied patch to ${n} files`, "Apply patch failed", () => api.applyPatch(patch), refresh);
  };
  return (
    <Dialog open onOpenChange={(o) => !o && close()}>
      <DialogContent className="max-w-lg">
        <DialogTitle>Apply patch</DialogTitle>
        <DialogDescription>
          {n === 1 ? "One file" : `${n} files`} from the clipboard, changed in the working tree; nothing is staged. {preview.applies ? HOW[preview.applies] : "It doesn't apply here:"}
        </DialogDescription>
        {preview.error && <pre className="mt-2 max-h-24 overflow-auto rounded-sm bg-active px-2 py-1 text-[11px] whitespace-pre-wrap text-muted-foreground">{preview.error}</pre>}
        <div role="list" aria-label="Files in the patch" className="mt-3 max-h-64 overflow-y-auto rounded-sm border border-border py-0.5">
          {preview.files.map((f) => (
            <div key={`${f.oldPath ?? ""}:${f.path}`} role="listitem" className="flex h-[26px] items-center gap-2 px-2 text-[12px]">
              <FileIcon path={f.path} />
              {f.oldPath && (
                <>
                  <PathLabel path={f.oldPath} className="max-w-[40%]" />
                  <span className="shrink-0 text-subtle">→</span>
                </>
              )}
              <PathLabel path={f.path} className="flex-1" />
              {f.additions === null ? <span className="shrink-0 text-[11px] text-subtle">binary</span> : <LineCounts file={f} />}
              <StatusLetter status={f.status} />
            </div>
          ))}
        </div>
        <div className="mt-4 flex justify-end gap-2">
          <Button variant="secondary" onClick={close}>
            Cancel
          </Button>
          <Button autoFocus disabled={!preview.applies} onClick={apply}>
            Apply
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
