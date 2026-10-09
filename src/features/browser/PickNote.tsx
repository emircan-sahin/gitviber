import { Send, X } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tip } from "@/components/ui/tooltip";
import type { BrowserPick } from "@/lib/api";
import { formatPick, type ShownAs } from "@/lib/browser/format";
import { toAgent } from "@/lib/browser/handoff";

/**
 * A picked element under the address, with a note for the agent: Enter sends both to the
 * worktree's agent. A bar, not a popover: one over the page would have it step aside.
 */
export function PickNote({ pick, device, root, onClose }: { pick: BrowserPick; device: ShownAs | null; root: string; onClose: () => void }) {
  const [note, setNote] = useState("");
  const send = () => {
    toAgent(root, formatPick(pick, note, device));
    onClose();
  };
  return (
    <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border px-2 text-[12px]">
      <Tip label={pick.selector}>
        <span className="max-w-[40%] shrink-0 truncate font-mono text-[11.5px] text-muted-foreground">
          {pick.components[0] ? `${pick.components[0]} · ` : ""}&lt;{pick.tag}&gt;
          {pick.text && <span className="text-subtle"> {pick.text.slice(0, 40)}</span>}
        </span>
      </Tip>
      <Input
        autoFocus
        aria-label="Note for the agent"
        placeholder="What should the agent do with it?"
        value={note}
        onChange={(e) => setNote(e.currentTarget.value)}
        onKeyDown={(e) => {
          // Enter that confirms an IME composition isn't a send (NameInput).
          if (e.key === "Enter" && !e.nativeEvent.isComposing && e.keyCode !== 229) send();
          else if (e.key === "Escape") onClose();
          else return;
          e.preventDefault();
        }}
        className="h-6 min-w-0 flex-1"
      />
      <Button type="button" size="sm" onClick={send}>
        <Send /> Send to Agent
      </Button>
      <Tip label="Discard">
        <Button type="button" variant="ghost" size="icon-sm" aria-label="Discard" onClick={onClose}>
          <X />
        </Button>
      </Tip>
    </div>
  );
}
