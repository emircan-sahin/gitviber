import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { api, type Commit } from "@/lib/api";
import { matchesCommand } from "@/lib/commands/keybindings";
import { refNameCheck } from "@/lib/git/refs";
import { NameHint } from "@/components/NameHint";
import { useSubmit } from "@/hooks/useGitAction";
import type { Actions } from "./commitActions";
import type { Messaging } from "./edits";

const fullMessage = (c: Commit) => (c.body.trim() ? `${c.subject}\n\n${c.body.trim()}` : c.subject);

/** The message for a reworded commit, or for commits squashed into one (theirs to start with, oldest first). */
export function MessageDialog({ messaging: m, onClose, onSubmit }: { messaging: Messaging; onClose: () => void; onSubmit: (message: string) => void }) {
  const [message, setMessage] = useState(() => (m.kind === "reword" ? [m.commit] : m.commits).map(fullMessage).join("\n\n"));
  const submit = () => {
    if (!message.trim()) return;
    onClose();
    onSubmit(message);
  };
  const count = m.kind === "squash" ? new Set([...m.shas, m.onto]).size : 1;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogTitle>{m.kind === "reword" ? "Reword commit" : `Squash ${count} commits`}</DialogTitle>
        <DialogDescription>
          {m.kind === "reword" ? (
            <>
              A new message for <span className="font-mono">{m.commit.shortSha}</span>; its changes stay as they are.
            </>
          ) : (
            <>
              These {count} commits become one
              {m.apart && (
                <>
                  {" "}
                  where <span className="font-mono">{m.onto.slice(0, 7)}</span> is
                </>
              )}
              , with this message.
            </>
          )}
        </DialogDescription>
        <form
          className="mt-4 flex flex-col gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
        >
          <Textarea
            autoFocus
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            onKeyDown={(e) => {
              // ⌘↵ saves, as in the commit box; a plain ↵ is a new line.
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                submit();
              }
            }}
            rows={8}
            className="font-mono text-[12px]"
            spellCheck={false}
          />
          <Button type="submit" className="self-end" disabled={!message.trim()}>
            {m.kind === "reword" ? "Reword" : "Squash"}
          </Button>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function NameDialog({ kind, commit, onClose, run }: { kind: "branch" | "tag"; commit: Commit; onClose: () => void; run: Actions["run"] }) {
  const [name, setName] = useState("");
  const [message, setMessage] = useState("");
  const check = refNameCheck(name, []);
  const n = check.name;
  const { pending, send } = useSubmit(onClose);
  const ready = !!n && !pending;
  const submit = () =>
    void send(() => (kind === "branch" ? run("Create branch", () => api.createBranchAt(n, commit.sha), `Switched to new branch ${n}`) : run("Create tag", () => api.createTag(n, commit.sha, message), `Tagged ${commit.shortSha} as ${n}`)));
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogTitle>{kind === "branch" ? "Create branch" : "Create tag"}</DialogTitle>
        <DialogDescription>
          At <span className="font-mono">{commit.shortSha}</span> {commit.subject}
          {kind === "branch" && ". You'll be switched to it; uncommitted changes come along."}
        </DialogDescription>
        <form
          className="mt-4 flex flex-wrap gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (ready) submit();
          }}
        >
          <Input autoFocus className="min-w-0 flex-1" value={name} onChange={(e) => setName(e.target.value)} placeholder={kind === "branch" ? "Branch name" : "Tag name, e.g. v1.2.0"} spellCheck={false} />
          <Button type="submit" disabled={!ready}>
            Create
          </Button>
          <NameHint {...check} className="-mt-0.5 w-full" />
          {kind === "tag" && (
            <Textarea
              value={message}
              onChange={(e) => setMessage(e.target.value)}
              // ⌘↵ submits from here too; a plain ↵ is a new line.
              onKeyDown={(e) => {
                if (matchesCommand("git.createTag", e.nativeEvent) && ready) {
                  e.preventDefault();
                  submit();
                }
              }}
              rows={3}
              placeholder="Message (optional): makes an annotated tag, which Push with tags sends along"
            />
          )}
        </form>
      </DialogContent>
    </Dialog>
  );
}
