import { ChevronDown, Tag, UploadCloud } from "lucide-react";
import { useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { api, errorMessage } from "@/lib/api";
import { remoteTags } from "@/lib/repo/remoteTags";

type Unpushed = { remote: string; tags: string[] } | { error: string } | null;

/**
 * The push button's menu: push with tags, and the local tags the remote lacks. Which those
 * are takes a network call, so it is asked only while the menu opens.
 */
export function PushMenu({ primary, disabled, onPush, onPushTags }: { primary: boolean; disabled: boolean; onPush: (tags: boolean) => void; onPushTags: (names: string[], remote: string) => void }) {
  const [unpushed, setUnpushed] = useState<Unpushed>(null);
  // Only the latest opening's answer is shown; a slower earlier one arrives out of order.
  const asked = useRef(0);
  const check = async () => {
    const id = ++asked.current;
    setUnpushed(null);
    let next: Unpushed;
    try {
      const [local, there] = await Promise.all([api.tags(), remoteTags()]);
      const have = new Set(there.names);
      next = { remote: there.remote, tags: local.filter((t) => !have.has(t)) };
    } catch (e) {
      next = { error: errorMessage(e) };
    }
    if (id === asked.current) setUnpushed(next);
  };
  const shown = (tags: string[]) => tags.slice(0, 3).join(", ") + (tags.length > 3 ? ` +${tags.length - 3}` : "");
  return (
    <DropdownMenu onOpenChange={(o) => o && void check()}>
      <DropdownMenuTrigger asChild>
        <Button variant={primary ? "default" : "secondary"} className={primary ? "w-5 rounded-l-none border-l border-l-black/20 px-0" : "w-5 rounded-l-none border-l-0 px-0"} disabled={disabled} aria-label="More push options">
          <ChevronDown className="size-3" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-72">
        <DropdownMenuItem onSelect={() => onPush(true)} className="items-start">
          <UploadCloud className="mt-0.5" />
          <span>
            Push with tags
            <span className="block text-[11px] text-muted-foreground">Also sends annotated tags on the pushed commits (--follow-tags). Lightweight tags stay; push them one by one from History.</span>
          </span>
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuLabel>Local tags</DropdownMenuLabel>
        {!unpushed ? (
          <div className="px-2 py-1.5 text-[12px] text-muted-foreground">Checking the remote…</div>
        ) : "error" in unpushed ? (
          <div className="px-2 py-1.5 text-[12px] text-muted-foreground">Couldn't compare tags with the remote: {unpushed.error}</div>
        ) : unpushed.tags.length ? (
          <DropdownMenuItem onSelect={() => onPushTags(unpushed.tags, unpushed.remote)}>
            <Tag />
            <span className="min-w-0 flex-1 truncate">
              Push {unpushed.tags.length === 1 ? "1 tag" : `${unpushed.tags.length} tags`} {unpushed.remote} lacks: <span className="font-mono">{shown(unpushed.tags)}</span>
            </span>
          </DropdownMenuItem>
        ) : (
          <div className="px-2 py-1.5 text-[12px] text-muted-foreground">{unpushed.remote} has every local tag.</div>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
