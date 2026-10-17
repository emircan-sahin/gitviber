import { FileIcon, FolderIcon } from "@/components/FileIcon";
import { IndentGuides, indent } from "@/components/IndentGuides";
import { pathTree } from "@/lib/ui/pathTree";

/** A section's files under it in the navigator, as folders and files; a file scrolls to its diff. */
export function GuideNavFiles({ paths, onGo }: { paths: string[]; onGo: (path: string) => void }) {
  return (
    <ul>
      {pathTree(paths, (p) => p).map((row) =>
        row.kind === "folder" ? (
          <li key={`d:${row.path}`} style={{ paddingLeft: indent(row.depth + 1) }} className="relative flex h-6 items-center gap-1.5 pr-2 text-[12px] text-muted-foreground">
            <IndentGuides depth={row.depth + 1} />
            <FolderIcon name={row.path.slice(row.path.lastIndexOf("/") + 1)} open />
            <span title={row.path} className="min-w-0 truncate">
              {row.label}
            </span>
          </li>
        ) : (
          <li key={row.item}>
            <button
              onClick={() => onGo(row.item)}
              title={row.item}
              style={{ paddingLeft: indent(row.depth + 1) }}
              className="relative flex h-6 w-full items-center gap-1.5 pr-2 text-left text-[12px] outline-none hover:bg-hover focus-visible:ring-1 focus-visible:ring-ring"
            >
              <IndentGuides depth={row.depth + 1} />
              <FileIcon path={row.item} />
              <span className="min-w-0 truncate">{row.name}</span>
            </button>
          </li>
        ),
      )}
    </ul>
  );
}
