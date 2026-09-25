import type { GitHubAccount } from "@/lib/api";
import { cn } from "@/lib/utils";

/** A failed load. With a cached list on screen, a note above it, not a blank panel. */
export function ListError({ error, loaded }: { error: string | null; loaded: boolean }) {
  if (!error) return null;
  if (!loaded) return <div className="px-4 py-6 text-center text-[12px] text-muted-foreground">{error}</div>;
  return <div className="mx-2 mb-1 rounded-sm bg-removed/10 px-2 py-1.5 text-[11.5px] text-removed">{error}</div>;
}

/** What an empty list says. `roomy`: the whole panel, not a pane, so it sits lower. */
export function EmptyNote({ roomy, children }: { roomy: boolean; children: React.ReactNode }) {
  return <div className={cn("px-4 text-center text-[12px] text-subtle", roomy ? "pt-16" : "py-3")}>{children}</div>;
}

export function SignedInAs({ account }: { account: GitHubAccount }) {
  return (
    <div className="shrink-0 border-t border-border px-3 py-1.5 text-[10.5px] text-subtle">
      Signed in as <span className="text-muted-foreground">{account.login}</span> via {account.source === "gh" ? "GitHub CLI" : "git credentials"}
    </div>
  );
}
