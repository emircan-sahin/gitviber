import type { ChangeStatus, FileChange } from "@/lib/api";
import { cn } from "@/lib/utils";
import { splitPath } from "@/lib/path";

const STATUS: Record<ChangeStatus, { letter: string; label: string; text: string; bg: string }> = {
  M: { letter: "M", label: "Modified", text: "text-modified", bg: "bg-modified-fill" },
  A: { letter: "A", label: "Added", text: "text-added", bg: "bg-added-fill" },
  "?": { letter: "U", label: "Untracked", text: "text-added", bg: "bg-added-fill" },
  D: { letter: "D", label: "Deleted", text: "text-removed", bg: "bg-removed-fill" },
  R: { letter: "R", label: "Renamed", text: "text-renamed", bg: "bg-renamed-fill" },
  C: { letter: "C", label: "Copied", text: "text-renamed", bg: "bg-renamed-fill" },
  T: { letter: "T", label: "Type changed", text: "text-modified", bg: "bg-modified-fill" },
  U: { letter: "!", label: "Conflict", text: "text-conflict", bg: "bg-conflict-fill" },
};

export function statusInfo(status: ChangeStatus) {
  return STATUS[status] ?? STATUS.M;
}

export function StatusLetter({ status, className }: { status: ChangeStatus; className?: string }) {
  const s = statusInfo(status);
  return (
    <span title={s.label} className={cn("w-3 shrink-0 text-center font-mono text-[11px] font-bold", s.text, className)}>
      {s.letter}
    </span>
  );
}

/** Solid label like GitButler's "Modified" chip. */
export function StatusPill({ status }: { status: ChangeStatus }) {
  const s = statusInfo(status);
  return <span className={cn("rounded-sm px-1.5 py-px text-[10.5px] font-semibold text-on-status", s.bg)}>{s.label}</span>;
}

export function LineCounts({ file, className }: { file: Pick<FileChange, "additions" | "deletions"> & { status?: ChangeStatus }; className?: string }) {
  // An untracked file with no counts at all wasn't read yet: status counts a few thousand per refresh.
  if (file.status === "?" && file.additions == null && file.deletions == null)
    return (
      <span title="Not counted yet" className={cn("shrink-0 font-mono text-[11px] text-subtle", className)}>
        ?
      </span>
    );
  if (file.additions == null && file.deletions == null) return null;
  return (
    <span className={cn("shrink-0 font-mono text-[11px] tabular-nums", className)}>
      {!!file.additions && <span className="text-added">+{file.additions}</span>}
      {!!file.additions && !!file.deletions && " "}
      {!!file.deletions && <span className="text-removed">-{file.deletions}</span>}
    </span>
  );
}

/** "dir/**name**": the directory gives way first; the file name keeps its full width when it can. */
export function PathLabel({ path, className }: { path: string; className?: string }) {
  const { dir, name } = splitPath(path);
  return (
    <span className={cn("flex min-w-0 items-baseline", className)} title={path}>
      {dir && <span className="min-w-0 truncate text-subtle">{dir}</span>}
      <span className="max-w-full shrink-0 truncate text-foreground">{name}</span>
    </span>
  );
}
