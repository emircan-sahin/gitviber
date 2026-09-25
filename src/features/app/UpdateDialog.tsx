import { CircleArrowUp } from "lucide-react";
import { useEffect } from "react";
import type { Components } from "react-markdown";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Tip } from "@/components/ui/tooltip";
import { github } from "@/lib/api";
import { useCommands } from "@/lib/commands/keybindings";
import { failed } from "@/lib/app/toast";
import { percent } from "@/lib/app/updateState";
import { checkForUpdates, downloadUpdate, RELEASES_URL, restartToUpdate, showUpdate, startUpdates, useUpdateMode, useUpdates, useUpdateShown } from "@/lib/app/updates";
import { followLink, MarkdownBody } from "@/features/viewer/MarkdownView";
import { markdownLink } from "@/lib/github/markdown";
import { REPO } from "./AboutDialog";

const NOTES_PREFIX = "release-notes-";
const components: Components = { a: markdownLink((href) => followLink(href, () => {}, NOTES_PREFIX)) };

/** A newer release's notes, and its download and restart. Also runs the checks and GitViber → Check for Updates…. */
export function UpdateDialog() {
  const open = useUpdateShown();
  const mode = useUpdateMode();
  const { release, download } = useUpdates();

  useEffect(() => startUpdates(), []);
  // Greyed out in the menu where the app doesn't update itself (dev builds).
  useCommands({ "app.checkForUpdates": mode ? () => void checkForUpdates(true) : undefined });

  const date = release?.date ? new Date(release.date).toLocaleDateString(undefined, { dateStyle: "medium" }) : null;
  const progress = typeof download === "object" && download ? percent(download) : null;

  return (
    <Dialog open={open && !!release} onOpenChange={showUpdate}>
      <DialogContent className="top-[12%] flex max-h-[76vh] max-w-lg flex-col">
        <DialogTitle>GitViber {release?.version} is available</DialogTitle>
        <DialogDescription>
          {date && `Released ${date}. `}
          {mode === "download" ? "This copy came from a .deb or .rpm package: install the new one from the Releases page." : "Nothing changes until you restart."}
        </DialogDescription>
        <div className="markdown mt-3 min-h-0 flex-1 overflow-y-auto rounded-sm border border-border bg-background px-3 py-2.5 text-[12.5px] select-text">
          {release?.notes.trim() ? <MarkdownBody text={release.notes} components={components} idPrefix={NOTES_PREFIX} repo={REPO} /> : <span className="text-subtle italic">No release notes.</span>}
        </div>
        {typeof download === "object" && download && (
          <div className="mt-3 h-1 overflow-hidden rounded-full bg-border">
            {/* Unknown size: a full bar that pulses rather than a fake percentage. */}
            <div className={progress === null ? "h-full animate-pulse bg-primary" : "h-full bg-primary transition-[width]"} style={{ width: `${progress ?? 100}%` }} />
          </div>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <Button variant="secondary" onClick={() => showUpdate(false)}>
            Later
          </Button>
          {mode === "download" ? (
            <Button onClick={() => github.openUrl(RELEASES_URL).catch(failed("Could not open the link"))}>Open Releases Page</Button>
          ) : download === "ready" ? (
            <Button onClick={() => void restartToUpdate()}>Restart to Update</Button>
          ) : (
            <Button disabled={download !== null} onClick={() => void downloadUpdate()}>
              {download === null ? "Download Update" : `Downloading…${progress === null ? "" : ` ${progress}%`}`}
            </Button>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

/** The status bar's version turns into this once a newer release is out. */
export function UpdateButton() {
  const { release, download } = useUpdates();
  if (!release) return null;
  const progress = typeof download === "object" && download ? percent(download) : null;
  const label = download === "ready" ? "Restart to Update" : download ? `Downloading update…${progress === null ? "" : ` ${progress}%`}` : `Update to v${release.version}`;
  return (
    <Tip label={download === "ready" ? `Restart into GitViber ${release.version}` : "What's new"}>
      <button
        onClick={() => (download === "ready" ? void restartToUpdate() : showUpdate())}
        className="flex items-center gap-1 font-medium text-primary tabular-nums hover:text-primary/80 focus-visible:text-primary/80"
      >
        <CircleArrowUp className="size-3" />
        {label}
      </button>
    </Tip>
  );
}
