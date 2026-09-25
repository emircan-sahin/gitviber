import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { api, type Commit } from "@/lib/api";
import { matchesCommand } from "@/lib/commands/keybindings";
import type { Actions } from "./commitActions";

const fullMessage = (c: Commit) => (c.body.trim() ? `${c.subject}\n\n${c.body.trim()}` : c.subject);

/** The message for a reworded commit, or for one squashed into its parent (both messages to start with). */
export function MessageDialog({ kind, commit, parent, onClose, onSubmit }: { kind: "reword" | "squash"; commit: Commit; parent?: Commit; onClose: () => void; onSubmit: (message: string) => void }) {
  const [message, setMessage] = useState(() => (kind === "reword" ? fullMessage(commit) : [parent && fullMessage(parent), fullMessage(commit)].filter(Boolean).join("\n\n")));
  const submit = () => {
    if (!message.trim()) return;
    onClose();
    onSubmit(message);
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogTitle>{kind === "reword" ? "Reword commit" : "Squash into parent"}</DialogTitle>
        <DialogDescription>
          {kind === "reword" ? (
            <>
              A new message for <span className="font-mono">{commit.shortSha}</span>; its changes stay as they are.
            </>
          ) : (
            <>
              <span className="font-mono">{commit.shortSha}</span> and the commit before it become one, with this message.
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
            {kind === "reword" ? "Reword" : "Squash"}
          </Button>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function NameDialog({ kind, commit, onClose, run }: { kind: "branch" | "tag"; commit: Commit; onClose: () => void; run: Actions["run"] }) {
  const [name, setName] = useState("");
  const [message, setMessage] = useState("");
  const submit = () => {
    const n = name.trim();
    onClose();
    if (kind === "branch") run("Create branch", () => api.createBranchAt(n, commit.sha), `Switched to new branch ${n}`);
    else run("Create tag", () => api.createTag(n, commit.sha, message), `Tagged ${commit.shortSha} as ${n}`);
  };
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
            if (name.trim()) submit();
          }}
        >
          <Input autoFocus className="min-w-0 flex-1" value={name} onChange={(e) => setName(e.target.value)} placeholder={kind === "branch" ? "Branch name" : "Tag name, e.g. v1.2.0"} spellCheck={false} />
          <Button type="submit" disabled={!name.trim()}>
            Create
          </Button>
          {kind === "tag" && (
            <Textarea
              value={message}
              onChange={(e) => setMessage(e.target.value)}
              // ⌘↵ submits from here too; a plain ↵ is a new line.
              onKeyDown={(e) => {
                if (matchesCommand("git.createTag", e.nativeEvent) && name.trim()) {
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
