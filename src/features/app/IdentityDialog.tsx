import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { api, errorMessage, type GitIdentity } from "@/lib/api";
import { identityAsks } from "@/lib/app/identity";
import { toast } from "@/lib/app/toast";

// Asked once per launch: a user who closes it may be setting it up their own way. A commit
// that fails for want of one asks again (askForIdentity); this is the ask it was closed on.
let dismissedAt: number | null = null;

/**
 * On a fresh machine the first commit fails with "Author identity unknown". This asks for the
 * missing parts when a repo opens, or when that commit's toast asks, as git resolves them
 * there (includeIf applies), and sets them globally.
 */
export function IdentityDialog({ root }: { root: string | null }) {
  const [missing, setMissing] = useState<GitIdentity | null>(null);
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [busy, setBusy] = useState(false);
  const asks = identityAsks.use();

  useEffect(() => {
    if (!root || dismissedAt === asks) return;
    let live = true;
    api.gitIdentity().then(
      ({ current, suggested }) => {
        if (!live || dismissedAt === asks || (current.name && current.email)) return;
        setName(suggested?.name ?? "");
        setEmail(suggested?.email ?? "");
        setMissing(current);
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, [root, asks]);

  if (!missing) return null;
  const close = () => {
    dismissedAt = asks;
    setMissing(null);
  };
  const needName = !missing.name;
  const needEmail = !missing.email;
  const ready = (!needName || name.trim()) && (!needEmail || email.trim());
  const save = async () => {
    setBusy(true);
    try {
      await api.setGitIdentity(needName ? name.trim() : null, needEmail ? email.trim() : null);
      toast("success", "git identity saved", "Set in your global git config (~/.gitconfig).");
      close();
    } catch (e) {
      toast("error", "Could not set your git identity", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open onOpenChange={(o) => !o && close()}>
      <DialogContent>
        <DialogTitle>Who's committing?</DialogTitle>
        <DialogDescription>git records a name and email with every commit, and yours {needName && needEmail ? "aren't" : "isn't fully"} set yet. They go in your global git config, so every repository uses them.</DialogDescription>
        <form
          className="mt-4 flex flex-col gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (ready && !busy) void save();
          }}
        >
          {needName && <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Your name" spellCheck={false} />}
          {needEmail && <Input autoFocus={!needName} type="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder="Your email" spellCheck={false} />}
          <div className="mt-2 flex justify-end gap-2">
            <Button type="button" variant="secondary" onClick={close}>
              Not now
            </Button>
            <Button type="submit" disabled={!ready || busy}>
              Save
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
