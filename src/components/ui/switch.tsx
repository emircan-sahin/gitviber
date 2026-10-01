import { useFieldLabel } from "@/lib/ui/fieldLabel";
import { cn } from "@/lib/utils";

function Switch({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  const { labelledBy, describedBy } = useFieldLabel();
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-labelledby={labelledBy}
      aria-describedby={describedBy}
      onClick={() => onChange(!checked)}
      className={cn("flex h-4 w-7 items-center rounded-full p-0.5 transition-colors", checked ? "bg-primary" : "bg-border-strong ring-1 ring-subtle ring-inset")}
    >
      <span className={cn("size-3 rounded-full bg-white shadow-sm transition-transform", checked && "translate-x-3")} />
    </button>
  );
}

export { Switch };
