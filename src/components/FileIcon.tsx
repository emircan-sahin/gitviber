import { fileIconUrl, folderIconUrl } from "@/lib/ui/icons";
import { cn } from "@/lib/utils";

export function FileIcon({ path, className }: { path: string; className?: string }) {
  return <img src={fileIconUrl(path)} alt="" draggable={false} className={cn("size-4 shrink-0", className)} />;
}

export function FolderIcon({ name, open, className }: { name: string; open: boolean; className?: string }) {
  return <img src={folderIconUrl(name, open)} alt="" draggable={false} className={cn("size-4 shrink-0", className)} />;
}
