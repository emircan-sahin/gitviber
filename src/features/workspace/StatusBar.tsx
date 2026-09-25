import { ArrowDown, ArrowUp, WrapText } from "lucide-react";
import { Tip } from "@/components/ui/tooltip";
import { useShortcut } from "@/lib/commands/keybindings";
import { languageLabel } from "@/lib/editor/language";
import { useShownLanguage } from "@/lib/editor/shownLanguage";
import type { OpenTarget } from "@/lib/app/openIn";
import type { Selection } from "@/lib/repo/selection";
import type { useRepo } from "@/lib/repo/useRepo";
import { codeFontName, LIGHT_SYNTAX_THEMES, SYNTAX_THEMES, updateSettings, useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { openAbout, useAbout } from "@/features/app/AboutDialog";
import { UpdateButton } from "@/features/app/UpdateDialog";
import { useUpdates } from "@/lib/app/updates";
import { changeTotals } from "@/features/changes/changeList";
import { lineInView } from "@/features/viewer/activeEditor";
import { OpenInButton } from "./OpenIn";

/** `active`: the open tab's selection, which Open In opens. */
export function StatusBar({ repo, reviewed, active }: { repo: ReturnType<typeof useRepo>; reviewed: number; active: Selection | undefined }) {
  const s = useSettings();
  const language = useShownLanguage();
  const wrapKey = useShortcut("editor.toggleWrap");
  const { status } = repo;
  const totals = changeTotals(repo);
  return (
    // The branch is in the top bar's breadcrumb already, so it isn't repeated here.
    <div className="flex h-6 shrink-0 items-center gap-3 border-t border-border bg-sidebar px-3 text-[11px] text-subtle">
      {status?.upstream && (
        <span className="flex items-center gap-1.5 font-mono">
          <span className={cn("flex items-center", status.ahead && "text-primary")}>
            <ArrowUp className="size-3" />
            {status.ahead}
          </span>
          <span className={cn("flex items-center", status.behind && "text-modified")}>
            <ArrowDown className="size-3" />
            {status.behind}
          </span>
        </span>
      )}
      {status?.operation && (
        <span className="font-semibold text-conflict uppercase">
          {status.operation.kind}
          {status.operation.step != null && ` ${status.operation.step}/${status.operation.total}`}
          {status.conflicted.length > 0 && ` · ${status.conflicted.length} ${status.conflicted.length === 1 ? "conflict" : "conflicts"}`}
        </span>
      )}
      {totals.files > 0 && (
        <span>
          {totals.files} changed · <span className="font-mono text-added">+{totals.add}</span> <span className="font-mono text-removed">-{totals.del}</span> · {reviewed}/
          {totals.files} reviewed
        </span>
      )}
      <span className="ml-auto">{s.dark ? SYNTAX_THEMES[s.syntaxTheme] : LIGHT_SYNTAX_THEMES[s.lightSyntaxTheme]}</span>
      <span>
        {codeFontName(s)} {s.codeFontSize}
      </span>
      <span>{s.sideBySide ? "Split" : "Unified"}</span>
      <Tip label="Word wrap" shortcut={wrapKey}>
        <button
          onClick={() => updateSettings({ wordWrap: !s.wordWrap })}
          className={cn("flex items-center gap-1 hover:text-foreground focus-visible:text-foreground", s.wordWrap && "text-primary hover:text-primary focus-visible:text-primary")}
        >
          <WrapText className="size-3" />
          Wrap
        </button>
      </Tip>
      {language && <span>{languageLabel(language)}</span>}
      <VersionInfo />
      <OpenInButton target={() => openTarget(active)} />
    </div>
  );
}

/** `v0.1.0 · macOS 15.5`; opens About, which can copy it for a bug report. Update to vX once there's one. */
function VersionInfo() {
  const about = useAbout();
  const { release } = useUpdates();
  if (release) return <UpdateButton />;
  if (!about) return null;
  return (
    <Tip label="About GitViber">
      <button onClick={openAbout} className="hover:text-foreground focus-visible:text-foreground">
        v{about.version} · {about.os}
      </button>
    </Tip>
  );
}

/** The open file while it's on disk, at the line in view; else the whole worktree. */
function openTarget(sel: Selection | undefined): OpenTarget {
  const change = sel?.kind === "unstaged" || sel?.kind === "staged" || sel?.kind === "conflict" || sel?.kind === "branch";
  const path = sel?.kind === "file" ? sel.path : change && sel.file.status !== "D" ? sel.file.path : null;
  return path === null ? { path: "" } : { path, line: lineInView(path) };
}
