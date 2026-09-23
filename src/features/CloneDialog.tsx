import { homeDir } from "@tauri-apps/api/path";
import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { useEffect, useState, useSyncExternalStore } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { api, CANCELLED, cancelNetwork, errorMessage, type NetOp, netOp, type Progress } from "@/lib/api";
import { cloneFolderName, cloneUrl } from "@/lib/clone";
import { updateSettings, useSettings } from "@/lib/settings";
import { toast } from "@/lib/toast";

let shown = false;
const listeners = new Set<() => void>();
function setShown(s: boolean) {
  shown = s;
  listeners.forEach((l) => l());
}
/** Welcome, File → Clone Repository… and the project switcher open it. */
export const openClone = () => setShown(true);

/** Clones a repository into a folder of the user's choice, then opens it. */
export function CloneDialog({ onCloned }: { onCloned: (path: string) => void }) {
  const visible = useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => shown,
  );
  const { cloneParent } = useSettings();
  const [url, setUrl] = useState("");
  // Follows the URL until the user types a name of their own.
  const [name, setName] = useState<string | null>(null);
  const [parent, setParent] = useState<string | null>(cloneParent);
  const [op, setOp] = useState<NetOp | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [error, setError] = useState<string | null>(null);

  const folder = name ?? cloneFolderName(cloneUrl(url));
  const ready = !op && !!url.trim() && !!folder.trim() && !!parent;

  // The last clone's folder, else the home folder.
  useEffect(() => {
    if (!visible || parent) return;
    if (cloneParent) setParent(cloneParent);
    else homeDir().then(setParent, () => {});
  }, [visible, parent, cloneParent]);

  const close = () => {
    setShown(false);
    setUrl("");
    setName(null);
    setError(null);
    setProgress(null);
  };

  const choose = async () => {
    const dir = await pickFolder({ directory: true, title: "Clone into", defaultPath: parent ?? undefined });
    if (typeof dir === "string") setParent(dir);
  };

  const clone = async () => {
    if (!ready || !parent) return;
    const o = netOp(setProgress);
    setOp(o);
    setError(null);
    setProgress(null);
    try {
      const path = await api.cloneRepo(cloneUrl(url), parent, folder.trim(), o);
      updateSettings({ cloneParent: parent });
      close();
      onCloned(path);
    } catch (e) {
      if (e === CANCELLED) toast("info", "Clone cancelled");
      else setError(errorMessage(e));
    } finally {
      setOp(null);
    }
  };

  return (
    // While cloning, only Cancel closes it: an Escape shouldn't silently stop a long clone.
    <Dialog open={visible} onOpenChange={(o) => !o && !op && close()}>
      <DialogContent className="max-w-lg">
        <DialogTitle>Clone Repository</DialogTitle>
        <DialogDescription>A URL git can clone, or owner/name for a GitHub repository.</DialogDescription>
        <form
          className="mt-4 flex flex-col gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            void clone();
          }}
        >
          <Input autoFocus value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://github.com/owner/name.git" disabled={!!op} spellCheck={false} />
          <div className="flex flex-col gap-1 text-[11.5px] text-muted-foreground">
            Clone into
            <div className="flex items-center gap-2">
              <span className="min-w-0 flex-1 truncate font-mono text-[11.5px] text-foreground" title={parent ?? undefined}>
                {parent ?? "…"}
              </span>
              <Button type="button" variant="secondary" size="sm" disabled={!!op} onClick={choose}>
                Choose…
              </Button>
            </div>
          </div>
          <label className="flex flex-col gap-1 text-[11.5px] text-muted-foreground">
            Folder name
            <Input value={folder} onChange={(e) => setName(e.target.value)} disabled={!!op} spellCheck={false} />
          </label>
          {op && (
            <div className="flex flex-col gap-1">
              <div className="h-1 overflow-hidden rounded-full bg-border">
                <div className="h-full bg-primary transition-[width]" style={{ width: `${progress?.percent ?? 0}%` }} />
              </div>
              <div className="text-[11px] text-subtle tabular-nums">
                {progress ? `${progress.phase}${progress.percent !== null ? ` ${progress.percent}%` : ""}` : "Connecting…"}
              </div>
            </div>
          )}
          {error && <pre className="max-h-40 overflow-auto font-sans text-[11.5px] whitespace-pre-wrap text-destructive select-text">{error}</pre>}
          <div className="mt-1 flex justify-end gap-2">
            {/* Past the transfer (checking out the files) it runs to the end. */}
            <Button type="button" variant="secondary" disabled={progress?.cancellable === false} onClick={() => (op ? void cancelNetwork(op) : close())}>
              Cancel
            </Button>
            <Button type="submit" disabled={!ready}>
              {op ? "Cloning…" : "Clone"}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
