import { ExternalLink, Link } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Tip } from "@/components/ui/tooltip";
import { copyLink, openOnGitHub } from "@/lib/github/url";

/** A PR or issue row's right-click menu; `extra` goes first. */
export function LinkMenu({ url, extra, children }: { url: string; extra?: React.ReactNode; children: React.ReactNode }) {
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent>
        {extra}
        <ContextMenuItem onSelect={() => copyLink(url)}>
          <Link /> Copy link
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => openOnGitHub(url)}>
          <ExternalLink /> Open on GitHub
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

/** A PR or issue view's copy-link action, beside "Open on GitHub". */
export function CopyLinkButton({ url }: { url: string }) {
  return (
    <Tip label="Copy link">
      <Button variant="ghost" size="icon-sm" aria-label="Copy link" onClick={() => copyLink(url)}>
        <Link />
      </Button>
    </Tip>
  );
}
