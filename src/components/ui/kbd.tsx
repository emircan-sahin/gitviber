import type * as React from "react";

function Kbd({ children }: { children: React.ReactNode }) {
  return <kbd className="rounded-sm border border-border-strong bg-background px-1.5 py-px font-mono text-[11px] text-foreground">{children}</kbd>;
}

export { Kbd };
