import { Copy, GitPullRequest, RefreshCw } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { copyText } from "@/lib/app/clipboard";
import { failed } from "@/lib/app/toast";
import { connectGitHub } from "@/lib/github/account";
import { signedOutNote } from "@/lib/github/connect";
import { gitHubOrigin } from "@/lib/github/githubCache";
import { IS_MAC } from "@/lib/platform";
import { isNotConnected } from "@/lib/api";

const LOGIN = "gh auth login";

/**
 * Shown while no GitHub login is found. Nothing here asks for one by itself: only its two buttons
 * look, and only the second may make the OS ask for a Keychain password.
 */
export function ConnectGitHub({ subject = "pull requests" }: { subject?: string }) {
  const [busy, setBusy] = useState<"gh" | "store" | null>(null);
  const [missed, setMissed] = useState<"gh" | "store" | null>(null);
  const look = async (store: boolean) => {
    setBusy(store ? "store" : "gh");
    setMissed(null);
    try {
      await connectGitHub(store);
    } catch (e) {
      // Still none: the screen stays, and says so. Anything else is shown as it is.
      if (isNotConnected(e)) setMissed(store ? "store" : "gh");
      else failed("Could not look for a login")(e);
    } finally {
      setBusy(null);
    }
  };
  return (
    <div className="px-5 pt-14 text-center">
      <GitPullRequest className="mx-auto size-7 text-subtle" />
      <div className="mt-3 text-[13px] font-medium">Sign in to GitHub to see {subject}</div>
      <div className="mt-1 text-[12px] leading-relaxed text-muted-foreground">{signedOutNote(gitHubOrigin(), subject)}</div>
      <div className="mt-5 space-y-2 text-left">
        <Step n={1} title="With the GitHub CLI (recommended)">
          {/* Homebrew is macOS's; elsewhere gh comes from the distro or cli.github.com. */}
          {!IS_MAC && <span className="mb-1 block text-[11.5px] text-muted-foreground">Install gh (cli.github.com), then:</span>}
          <div className="flex items-center gap-1">
            <code className="min-w-0 flex-1 rounded-sm bg-background px-2 py-1 font-mono text-[11.5px]">{IS_MAC ? `brew install gh && ${LOGIN}` : LOGIN}</code>
            <Button variant="ghost" size="icon-sm" aria-label="Copy command" onClick={() => copyText(IS_MAC ? `brew install gh && ${LOGIN}` : LOGIN, "Command copied")}>
              <Copy />
            </Button>
          </div>
          <Button variant="secondary" size="sm" className="mt-2" disabled={busy !== null} onClick={() => look(false)}>
            <RefreshCw className={busy === "gh" ? "animate-spin" : undefined} /> I've signed in, check again
          </Button>
          {missed === "gh" && <span className="mt-1.5 block text-[11.5px] text-muted-foreground">Still no login from gh.</span>}
        </Step>
        <Step n={2} title="Or the login git stored for HTTPS">
          <span className="block text-[11.5px] text-muted-foreground">
            Reads what git saved for github.com ({IS_MAC ? "Keychain, GitHub Desktop, " : "a credential helper, "}Git Credential Manager). {IS_MAC && "macOS may ask for your Keychain password."}
          </span>
          <Button variant="secondary" size="sm" className="mt-2" disabled={busy !== null} onClick={() => look(true)}>
            Use it
          </Button>
          {missed === "store" && <span className="mt-1.5 block text-[11.5px] text-muted-foreground">Git has no login stored for github.com.</span>}
        </Step>
      </div>
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
