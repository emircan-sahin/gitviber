import { CircleAlert, Download, FolderOpen, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { browserApi, externalApi } from "@/lib/api";
import { failed } from "@/lib/app/toast";
import { type BrowserDownload, downloadText } from "@/lib/browser/downloads";
import { dismissDownload, useDownloads } from "@/lib/browser/store";

/** What the tab's page downloads, one row each under the address: how far it is, where it went, Cancel and Show in Finder. */
export function DownloadBar({ id }: { id: string }) {
  const downloads = useDownloads(id);
  if (!downloads.length) return null;
  return (
    <div className="flex shrink-0 flex-col border-b border-border">
      {downloads.map((d) => (
        <Row key={d.key} id={id} d={d} />
      ))}
    </div>
  );
}

function Row({ id, d }: { id: string; d: BrowserDownload }) {
  const going = d.state === "going";
  const Icon = d.state === "failed" ? CircleAlert : Download;
  return (
    <div className="flex h-7 items-center gap-2 px-3 text-xs" role={going ? undefined : "status"}>
      <Icon className={d.state === "failed" ? "size-3.5 shrink-0 text-destructive" : "size-3.5 shrink-0 text-muted-foreground"} />
      <span className="min-w-0 truncate font-medium" title={d.name}>
        {d.name}
      </span>
      <span className="min-w-0 flex-1 truncate text-muted-foreground" title={d.state === "failed" ? (d.why ?? undefined) : (d.path ?? undefined)}>
        {downloadText(d)}
      </span>
      {going && (
        <Button type="button" variant="ghost" size="sm" className="h-5 px-1.5 text-xs" onClick={() => void browserApi.cancelDownload(d.key).catch(failed("Could not cancel the download"))}>
          Cancel
        </Button>
      )}
      {d.state === "done" && d.path && (
        <Tip label="Show in Finder">
          <Button type="button" variant="ghost" size="sm" className="h-5 px-1.5" aria-label="Show in Finder" onClick={() => void externalApi.reveal(d.path ?? "").catch(failed("Could not show the file"))}>
            <FolderOpen />
          </Button>
        </Tip>
      )}
      {!going && (
        <Tip label="Dismiss">
          <Button type="button" variant="ghost" size="sm" className="h-5 px-1" aria-label="Dismiss" onClick={() => dismissDownload(id, d.key)}>
            <X />
          </Button>
        </Tip>
      )}
    </div>
  );
}
