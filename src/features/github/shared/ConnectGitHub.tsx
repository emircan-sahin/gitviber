import { GitPullRequest, Terminal } from "lucide-react";
import { Button } from "@/components/ui/button";

/** No token from the GitHub CLI or git's credential store: explain the two ways to connect. */
export function ConnectGitHub({ onRetry, subject = "pull requests" }: { onRetry: () => void; subject?: string }) {
  return (
    <div className="px-5 pt-14 text-center">
      <GitPullRequest className="mx-auto size-7 text-subtle" />
      <div className="mt-3 text-[13px] font-medium">Connect GitHub to see {subject}</div>
      <div className="mt-1 text-[12px] leading-relaxed text-muted-foreground">
        GitViber uses the login you already have. It never stores a token itself.
      </div>
      <div className="mt-5 space-y-2 text-left">
        <Step n={1} title="With the GitHub CLI (recommended)">
          <code className="block rounded-sm bg-background px-2 py-1 font-mono text-[11.5px]">brew install gh && gh auth login</code>
        </Step>
        <Step n={2} title="Or push once over HTTPS">
          <span className="text-[11.5px] text-muted-foreground">If git has stored your github.com login (Keychain, GitHub Desktop, Git Credential Manager), it's picked up automatically.</span>
        </Step>
      </div>
      <Button variant="secondary" size="sm" className="mt-5" onClick={onRetry}>
        <Terminal /> I've signed in — retry
      </Button>
    </div>
  );
}

function Step({ n, title, children }: { n: number; title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-md border border-border bg-panel p-2.5">
      <div className="mb-1.5 flex items-center gap-2 text-[12px] font-medium">
        <span className="flex size-4 items-center justify-center rounded-sm bg-elevated font-mono text-[10px]">{n}</span>
        {title}
      </div>
      {children}
    </div>
  );
}
