import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { type AskPrompt, answerPrompt, isBackgroundOp } from "@/lib/api";
import { promptForm } from "@/lib/git/askpass";

/**
 * What git or ssh asks during a network command (askpass.rs): a login, an SSH key's passphrase,
 * a new host's key. One at a time, in the order asked; a prompt whose command stopped or timed
 * out leaves on its own.
 */
export function PromptDialog() {
  const [queue, setQueue] = useState<AskPrompt[]>([]);

  useEffect(() => {
    const drop = (id: number) => setQueue((q) => q.filter((p) => p.id !== id));
    const asked = listen<AskPrompt>("askpass", ({ payload: p }) => {
      if (isBackgroundOp(p.op)) void answerPrompt(p.id, null).catch(() => {});
      else setQueue((q) => [...q, p]);
    });
    const done = listen<number>("askpass-done", ({ payload }) => drop(payload));
    return () => {
      void asked.then((off) => off());
      void done.then((off) => off());
    };
  }, []);

  const current = queue[0];
  if (!current) return null;
  const reply = (answer: string | null) => {
    void answerPrompt(current.id, answer).catch(() => {});
    setQueue((q) => q.filter((p) => p.id !== current.id));
  };
  // Keyed, so nothing typed for one prompt is left in the next.
  return <Prompt key={current.id} prompt={current} onReply={reply} />;
}

function Prompt({ prompt, onReply }: { prompt: AskPrompt; onReply: (answer: string | null) => void }) {
  const [value, setValue] = useState("");
  const form = promptForm(prompt);
  const details = form.type === "confirm" || form.type === "notice" ? form.details : null;

  return (
    <Dialog open onOpenChange={(open) => !open && onReply(null)}>
      <DialogContent className="max-w-lg">
        <DialogTitle>{form.title}</DialogTitle>
        <DialogDescription>
          {form.type === "confirm" && form.hostKey
            ? `${prompt.label} is connecting to a host ssh hasn't seen before. Trust it only if the fingerprint matches the one your provider publishes.`
            : form.type === "confirm"
              ? form.question
              : `${prompt.label} is asking.`}
        </DialogDescription>
        {details && <pre className="mt-3 max-h-40 overflow-auto rounded-md border border-border bg-panel p-2.5 font-mono text-[11px] whitespace-pre-wrap text-subtle select-text">{details}</pre>}
        {form.type === "text" || form.type === "secret" ? (
          <form
            className="mt-4 flex flex-col gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              onReply(value);
            }}
          >
            <label className="text-[12px] text-muted-foreground break-all">{form.label}</label>
            <Input autoFocus type={form.type === "secret" ? "password" : "text"} value={value} onChange={(e) => setValue(e.target.value)} spellCheck={false} autoComplete="off" autoCapitalize="off" autoCorrect="off" />
            {form.type === "secret" && <p className="text-[11px] text-subtle">Passed to git once; GitViber keeps nothing. Your credential helper may save it, as in a terminal.</p>}
            <div className="mt-2 flex justify-end gap-2">
              <Button type="button" variant="secondary" onClick={() => onReply(null)}>
                Cancel
              </Button>
              <Button type="submit">Continue</Button>
            </div>
          </form>
        ) : (
          <div className="mt-4 flex justify-end gap-2">
            {/* Cancel comes first, so Enter on an unread host key doesn't trust it. */}
            <Button variant="secondary" onClick={() => onReply(null)}>
              {form.type === "notice" ? "Dismiss" : "Cancel"}
            </Button>
            {form.type === "confirm" && <Button onClick={() => onReply("yes")}>{form.hostKey ? "Connect" : "Yes"}</Button>}
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
