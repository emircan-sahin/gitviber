import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { errorMessage, type Issue, issues, type Target } from "@/lib/api";
import { toast } from "@/lib/app/toast";
import { MarkdownInput } from "@/features/github/shared/MarkdownInput";
import { notifyIssuesChanged } from "@/features/github/shared/changed";

/** `repo`: owner/name of `target`, which is null for origin. */
export function CreateIssueDialog({ target, repo, onClose, onCreated }: { target: Target; repo: string; onClose: () => void; onCreated: (i: Issue) => void }) {
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    try {
      onCreated(await issues.create(target, title.trim(), body));
      toast("success", "Issue created");
      notifyIssuesChanged();
    } catch (e) {
      toast("error", "Could not create issue", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogTitle>New issue</DialogTitle>
        <DialogDescription>{target ? `Opened on ${target}.` : "Opened on the repository's GitHub page."}</DialogDescription>
        <form
          className="mt-4 space-y-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (title.trim()) submit();
          }}
        >
          <Input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Title" />
          <MarkdownInput pull={{ url: `https://github.com/${repo}`, number: null }} value={body} onChange={(e) => setBody(e.target.value)} placeholder="Description (markdown)" rows={8} />
          <div className="flex justify-end">
            <Button type="submit" disabled={busy || !title.trim()}>
              {busy ? "Creating…" : "Create"}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
