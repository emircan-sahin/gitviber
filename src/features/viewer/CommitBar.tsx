import { ExternalLink, GitCommitHorizontal } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { fullDate, relativeTime } from "@/lib/format";
import { githubRefs, markdownLink } from "@/lib/github/markdown";
import { repoOfCommitUrl } from "@/lib/github/permalink";
import { openOnGitHub } from "@/lib/github/url";
import { copyText } from "@/lib/app/clipboard";
import { CopyLinkButton } from "@/features/github/shared/LinkMenu";
import { SignatureBadge, TrailerChips, useCommitDetails } from "@/features/history/commitDetails";

const Link = markdownLink(openOnGitHub);

/** #123 and @login as links, as GitHub shows a commit message; plain without a GitHub repo. */
function Refs({ text, repo }: { text: string; repo: string | null }) {
  if (!repo) return text;
  return githubRefs(text, repo).map((s, i) =>
    s.href ? (
      <Link key={i} href={s.href} className="text-primary hover:underline">
        {s.text}
      </Link>
    ) : (
      s.text
    ),
  );
}

/** `url`: the commit on GitHub, which may be a fork's original; `web`: origin's page. */
export function CommitBar({ commit, url, web }: { commit: import("@/lib/api").Commit; url?: string; web: string | null }) {
  const [open, setOpen] = useState(false);
  const details = useCommitDetails(commit.sha);
  const repo = url ? repoOfCommitUrl(url) : web;
  return (
    <div className="shrink-0 border-b border-border bg-panel px-3 py-2">
      <div className="flex items-center gap-2">
        <GitCommitHorizontal className="size-3.5 shrink-0 text-primary" />
        <span className="truncate text-[12.5px] font-semibold select-text">
          <Refs text={commit.subject} repo={repo} />
        </span>
        <div className="ml-auto flex shrink-0 items-center gap-2 text-[11.5px] text-muted-foreground">
          <span>{commit.authorName}</span>
          <span className="text-subtle">·</span>
          <span title={fullDate(commit.timestamp)}>{relativeTime(commit.timestamp)}</span>
          {/* Rebased or cherry-picked: it landed later than it was written, maybe by someone else. */}
          {relativeTime(commit.committedAt) !== relativeTime(commit.timestamp) && (
            <>
              <span className="text-subtle">·</span>
              <span title={fullDate(commit.committedAt)}>
                committed {relativeTime(commit.committedAt)}
                {commit.committerName !== commit.authorName && ` by ${commit.committerName}`}
              </span>
            </>
          )}
          {details && <SignatureBadge details={details} />}
          <button className="rounded-sm bg-elevated px-1.5 py-px font-mono text-[11px] hover:text-foreground focus-visible:text-foreground" onClick={() => copyText(commit.sha, "Commit SHA copied")}>
            {commit.shortSha}
          </button>
          {commit.body && (
            <button className="font-medium text-primary hover:underline" onClick={() => setOpen(!open)}>
              {open ? "less" : "more"}
            </button>
          )}
          {url && (
            <div className="-my-1 flex items-center">
              <CopyLinkButton url={url} />
              <Tip label="Open on GitHub">
                <Button variant="ghost" size="icon-sm" aria-label="Open on GitHub" onClick={() => openOnGitHub(url)}>
                  <ExternalLink />
                </Button>
              </Tip>
            </div>
          )}
        </div>
      </div>
      {details && <TrailerChips details={details} className="mt-1.5 pl-5.5" />}
      {open && (
        <pre className="mt-2 max-h-48 overflow-auto pl-5.5 font-sans text-[12px] leading-relaxed whitespace-pre-wrap text-muted-foreground select-text">
          <Refs text={commit.body} repo={repo} />
        </pre>
      )}
    </div>
  );
}
