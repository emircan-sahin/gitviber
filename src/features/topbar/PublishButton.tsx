import { Check, ChevronDown, UploadCloud } from "lucide-react";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { DisabledTip, Tip } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

/** Publishes to `preferred` (see git/remote.rs `publish_remote`); with several remotes, any can be picked. */
export function PublishButton({ remotes, preferred, disabled, onPublish }: { remotes: string[]; preferred: string | null; disabled: boolean; onPublish: (remote: string) => void }) {
  if (!remotes.length) {
    return (
      <DisabledTip label="This repository has no remote. Add one (git remote add origin <url>) to publish." disabled>
        <Button disabled>
          <UploadCloud /> Publish
        </Button>
      </DisabledTip>
    );
  }
  const menu = (
    <DropdownMenuContent align="end" className="w-56">
      <DropdownMenuLabel>Publish to</DropdownMenuLabel>
      {remotes.map((r) => (
        <DropdownMenuItem key={r} onSelect={() => onPublish(r)}>
          <UploadCloud /> {r}
          {r === preferred && <Check className="ml-auto" />}
        </DropdownMenuItem>
      ))}
    </DropdownMenuContent>
  );
  if (!preferred) {
    return (
      <DropdownMenu>
        <Tip label="Choose a remote to push this branch to and track it">
          <DropdownMenuTrigger asChild>
            <Button disabled={disabled}>
              <UploadCloud /> Publish <ChevronDown className="size-3" />
            </Button>
          </DropdownMenuTrigger>
        </Tip>
        {menu}
      </DropdownMenu>
    );
  }
  return (
    <div className="flex">
      <Tip label={`Push this branch to ${preferred} and track it`}>
        <Button className={cn(remotes.length > 1 && "rounded-r-none")} disabled={disabled} onClick={() => onPublish(preferred)}>
          <UploadCloud /> Publish
        </Button>
      </Tip>
      {remotes.length > 1 && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button className="w-5 rounded-l-none border-l border-l-black/20 px-0" disabled={disabled} aria-label="Publish to another remote">
              <ChevronDown className="size-3" />
            </Button>
          </DropdownMenuTrigger>
          {menu}
        </DropdownMenu>
      )}
    </div>
  );
}
