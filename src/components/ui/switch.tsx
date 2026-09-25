import { cn } from "@/lib/utils";

function Switch({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={cn("flex h-4 w-7 items-center rounded-full p-0.5 transition-colors", checked ? "bg-primary" : "bg-border-strong")}
    >
      <span className={cn("size-3 rounded-full bg-white shadow-sm transition-transform", checked && "translate-x-3")} />
    </button>
  );
}

export { Switch };
