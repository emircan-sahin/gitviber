import { ShieldAlert, ShieldCheck, ShieldX } from "lucide-react";
import { useEffect, useState } from "react";
import { Tip } from "@/components/ui/tooltip";
import { api, type CommitDetails } from "@/lib/api";
import { cn } from "@/lib/utils";

// A commit's signature and trailers never change, and checking a signature runs gpg or ssh:
// ask once per commit, not per file opened in it.
const details = new Map<string, Promise<CommitDetails>>();

/** Signature status and trailers of the commit shown in the header; null while loading. */
export function useCommitDetails(sha: string) {
  const [d, setD] = useState<{ sha: string; details: CommitDetails } | null>(null);
  useEffect(() => {
    let alive = true;
    let p = details.get(sha);
    if (!p) {
      p = api.commitDetails(sha);
      details.set(sha, p);
      p.catch(() => details.delete(sha));
    }
    p.then(
      (x) => alive && setD({ sha, details: x }),
      () => {},
    );
    return () => {
      alive = false;
    };
  }, [sha]);
  return d?.sha === sha ? d.details : null;
}

const SIGNATURES: Record<string, { label: string; tone: string; icon: typeof ShieldCheck; tip: (signer: string) => string }> = {
  G: { label: "Verified", tone: "text-added", icon: ShieldCheck, tip: (s) => `Good signature from ${s}` },
  U: { label: "Signed", tone: "text-added", icon: ShieldCheck, tip: (s) => `Good signature from ${s}, whose key isn't marked as trusted` },
  X: { label: "Signed", tone: "text-modified", icon: ShieldAlert, tip: (s) => `Good signature from ${s}, but the signature has expired` },
  Y: { label: "Signed", tone: "text-modified", icon: ShieldAlert, tip: (s) => `Good signature from ${s}, made with a key that has since expired` },
  R: { label: "Revoked key", tone: "text-removed", icon: ShieldX, tip: (s) => `Signed by ${s} with a key that has been revoked` },
  B: { label: "Bad signature", tone: "text-removed", icon: ShieldX, tip: () => "The signature doesn't match this commit" },
  E: { label: "Signed", tone: "text-subtle", icon: ShieldAlert, tip: () => "Signed, but git couldn't check it: the key is missing or verification isn't set up" },
};

/** Nothing for an unsigned commit, unless commit.gpgSign says it should have been signed. */
export function SignatureBadge({ details: d }: { details: CommitDetails }) {
  const sig =
    SIGNATURES[d.signature] ??
    (d.signExpected ? { label: "Unsigned", tone: "text-modified", icon: ShieldAlert, tip: () => "commit.gpgSign is on, but this commit has no signature" } : null);
  if (!sig) return null;
  const Icon = sig.icon;
  return (
    // Focusable so the keyboard can read the tooltip too (Radix opens it on focus).
    <Tip label={sig.tip(d.signer || "an unknown key")}>
      <span tabIndex={0} className={cn("flex items-center gap-1 rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-ring", sig.tone)}>
        <Icon className="size-3.5" />
        {sig.label}
      </span>
    </Tip>
  );
}

/** Co-authored-by, Signed-off-by and the like, with the name and not the email. */
export function TrailerChips({ details: d, className }: { details: CommitDetails; className?: string }) {
  if (!d.trailers.length) return null;
  return (
    <div className={cn("flex flex-wrap gap-1", className)}>
      {d.trailers.map(([key, value], i) => (
        <Tip key={i} label={`${key}: ${value}`}>
          <span tabIndex={0} className="flex h-5 max-w-72 items-center gap-1 rounded-[3px] bg-elevated px-1.5 text-[11px] outline-none focus-visible:ring-1 focus-visible:ring-ring">
            <span className="shrink-0 text-subtle">{key}</span>
            <span className="truncate text-muted-foreground">{value.replace(/\s*<[^>]*>$/, "") || value}</span>
          </span>
        </Tip>
      ))}
    </div>
  );
}
