import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { createStore } from "@/lib/store";
import { moveAlong } from "./stacked";

/** What a rewrite or rebase asks before it runs: `branches` are the local ones on the commits it replays. */
export interface StackedAsk {
  title: string;
  /** Warnings to read first (pushed commits, a drop), if any. */
  message: string;
  okLabel: string;
  branches: string[];
  /** Checked at first: rebase.updateRefs is set. */
  checked: boolean;
}

interface Pending extends StackedAsk {
  answer: (moveBranches: boolean | null) => void;
}

const shown = createStore<Pending | null>(null);

/** Asks whether the branches move along; null when cancelled. Opened from History and the branch picker alike. */
export const askStacked = (ask: StackedAsk) =>
  new Promise<boolean | null>((resolve) => {
    // A second one while one is open (it can't be: the first is modal) would cancel it.
    shown.get()?.answer(null);
    shown.set({ ...ask, answer: resolve });
  });

export function StackedDialog() {
  const pending = shown.use();
  if (!pending) return null;
  const reply = (v: boolean | null) => {
    shown.set(null);
    pending.answer(v);
  };
  return <Ask key={pending.title} pending={pending} reply={reply} />;
}

function Ask({ pending, reply }: { pending: Pending; reply: (v: boolean | null) => void }) {
  const [move, setMove] = useState(pending.checked);
  return (
    <Dialog open onOpenChange={(o) => !o && reply(null)}>
      <DialogContent className="max-w-md">
        <DialogTitle>{pending.title}</DialogTitle>
        {pending.message && <DialogDescription className="whitespace-pre-wrap">{pending.message}</DialogDescription>}
        <label className="mt-3 flex items-start gap-2 text-[12px]">
          <input type="checkbox" checked={move} onChange={(e) => setMove(e.target.checked)} className="mt-0.5 accent-primary" />
          <span>
            Also move {moveAlong(pending.branches)}
            <span className="block text-[11.5px] text-muted-foreground">They point at commits this replays; moved, they stay on the new ones. Branches checked out in another worktree stay.</span>
          </span>
        </label>
        <div className="mt-4 flex justify-end gap-2">
          <Button variant="secondary" onClick={() => reply(null)}>
            Cancel
          </Button>
          <Button autoFocus onClick={() => reply(move)}>
            {pending.okLabel}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
