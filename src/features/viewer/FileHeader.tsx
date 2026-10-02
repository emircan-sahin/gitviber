import { Copy } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { FileIcon } from "@/components/FileIcon";
import { PathLabel } from "@/components/StatusBadge";
import { copyText } from "@/lib/app/clipboard";

/**
 * A file view header's start: the file's icon, its path, and a button that copies it. `shown`:
 * the path as shown, when it isn't the one copied (a vault file shows under its vault's name).
 */
export function FileHeaderPath({ path, shown = path }: { path: string; shown?: string }) {
  return (
    <>
      <FileIcon path={shown} />
      <PathLabel path={shown} className="min-w-0 text-[12px]" />
      <Tip label="Copy path">
        <button className="hit-area text-subtle hover:text-foreground focus-visible:text-foreground" onClick={() => copyText(path, "Path copied")}>
          <Copy className="size-3" />
        </button>
      </Tip>
    </>
  );
}

/** What a view says in place of its content: why it can't show, and maybe a way out. */
export function Placeholder({ title, detail, action }: { title: string; detail?: string; action?: { label: string; run: () => void } }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center">
      <div className="text-[12.5px] text-muted-foreground">{title}</div>
      {detail && <pre className="max-w-xl font-mono text-[11.5px] whitespace-pre-wrap text-subtle select-text">{detail}</pre>}
      {action && (
        <Button variant="secondary" size="sm" className="mt-1" onClick={action.run}>
          {action.label}
        </Button>
      )}
    </div>
  );
}
