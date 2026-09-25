import { Plus, UserPlus, X } from "lucide-react";
import { useEffect, useState } from "react";
import { Input } from "@/components/ui/input";
import { Popover, PopoverAnchor, PopoverContent } from "@/components/ui/popover";
import { Tip } from "@/components/ui/tooltip";
import { api } from "@/lib/api";
import { pointerMoved } from "@/lib/ui/pointer";
import { cn } from "@/lib/utils";
import { usePickerIndex } from "@/hooks/usePickerIndex";

/** A commit option that's on, shown so it isn't forgotten; × turns it off. */
export function OptionChip({ label, tip, warn, onRemove, children }: { label: string; tip: string; warn?: boolean; onRemove: () => void; children: React.ReactNode }) {
  return (
    <Tip label={tip}>
      <span
        className={cn(
          "flex h-5 max-w-full items-center gap-1 rounded-[3px] pr-0.5 pl-1.5 text-[11px] [&_svg]:size-3 [&_svg]:shrink-0",
          warn ? "bg-modified/15 text-modified" : "bg-elevated text-muted-foreground",
        )}
      >
        {children}
        <span className="truncate">{label}</span>
        <button aria-label={`Remove ${label}`} onClick={onRemove} className="flex size-4 items-center justify-center rounded-sm opacity-70 outline-none hover:bg-active focus-visible:bg-active hover:opacity-100 focus-visible:ring-1 focus-visible:ring-ring">
          <X />
        </button>
      </span>
    </Tip>
  );
}

// GitHub reads a co-author only in this form.
const CO_AUTHOR = /^[^<>\n]+ <[^<>\s]+@[^<>\s]+>$/;
const nameOf = (author: string) => author.replace(/\s*<[^>]*>$/, "") || author;

export function CoAuthorChip({ author, onRemove }: { author: string; onRemove: () => void }) {
  return (
    <OptionChip label={nameOf(author)} tip={`Co-authored-by: ${author}`} onRemove={onRemove}>
      <UserPlus />
    </OptionChip>
  );
}

/** Picks a co-author from recent authors and co-authors, or takes one typed as "Name <email>". */
export function CoAuthorPicker({
  open,
  onOpenChange,
  taken,
  onAdd,
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  taken: string[];
  onAdd: (author: string) => void;
  children: React.ReactNode;
}) {
  const [authors, setAuthors] = useState<string[] | null>(null);
  const [query, setQuery] = useState("");

  const q = query.trim();
  const found = (authors ?? []).filter((a) => !taken.includes(a) && a.toLowerCase().includes(q.toLowerCase())).slice(0, 8);
  const typed = CO_AUTHOR.test(q) && !taken.includes(q) && !found.includes(q) ? [q] : [];
  const options = [...typed, ...found];
  const { index, setIndex, move } = usePickerIndex(options.length);
  useEffect(() => {
    if (!open) return;
    setQuery("");
    setIndex(0);
    api.recentAuthors().then(setAuthors, () => setAuthors([]));
  }, [open]);
  const add = (a: string) => {
    onAdd(a);
    onOpenChange(false);
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") move(1);
    else if (e.key === "ArrowUp") move(-1);
    else if (e.key === "Enter" && options[index]) add(options[index]);
    else return;
    e.preventDefault();
  };

  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverAnchor className="flex">{children}</PopoverAnchor>
      <PopoverContent side="top" align="end" className="flex w-80 flex-col p-1" onKeyDown={onKeyDown}>
        <Input
          autoFocus
          placeholder="Search recent authors, or Name <email>"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setIndex(0);
          }}
          spellCheck={false}
        />
        <div className="mt-1 max-h-56 overflow-y-auto" onMouseLeave={(e) => pointerMoved(e) && setIndex(-1)}>
          {options.map((a, i) => (
            <div
              key={a}
              role="option"
              aria-selected={i === index}
              onMouseMove={(e) => pointerMoved(e) && setIndex(i)}
              onClick={() => add(a)}
              className={cn("flex h-7 cursor-pointer items-center gap-2 rounded-sm px-2 text-[12px] select-none", i === index && "bg-primary text-primary-foreground")}
            >
              {typed[0] === a && <Plus className="size-3.5 shrink-0" />}
              <span className="truncate">{nameOf(a)}</span>
              <span className="ml-auto truncate text-[11px] opacity-70">{a.slice(nameOf(a).length).trim()}</span>
            </div>
          ))}
          {!options.length && (
            <div className="px-2 py-2 text-[11.5px] text-subtle">{authors === null ? "Loading…" : q ? "No match. Type it as Name <email@example.com>." : "No recent authors yet. Type Name <email@example.com>."}</div>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
