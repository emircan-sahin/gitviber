import { cn } from "@/lib/utils";

/** The line under a name field, from refNameCheck: what the typed name becomes, or that it's taken. */
export function NameHint({ hint, taken }: { hint: string | null; taken: boolean }) {
  if (!hint) return null;
  return <div className={cn("mt-1.5 text-[11.5px]", taken ? "text-removed" : "text-muted-foreground")}>{hint}</div>;
}
