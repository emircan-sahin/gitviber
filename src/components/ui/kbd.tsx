import type * as React from "react";
import { chordKeys } from "@/lib/commands/commands";

function Kbd({ children }: { children: React.ReactNode }) {
  return <kbd className="rounded-sm border border-border-strong bg-background px-1.5 py-px font-mono text-[11px] text-foreground">{children}</kbd>;
}

/** One keycap per key, in the system font, whose ⌘ ⇧ ⌥ ⌃ glyphs read at this size where a monospace font's run together. */
function Keycaps({ chord }: { chord: string }) {
  return (
    <span className="flex gap-[3px]">
      {chordKeys(chord).map((k, i) => (
        <kbd
          key={i}
          className="flex h-[19px] min-w-[19px] items-center justify-center rounded-[4px] border border-b-2 border-border-strong bg-background px-1 [font-family:system-ui] text-[11.5px] leading-none font-medium text-foreground"
        >
          {k}
        </kbd>
      ))}
    </span>
  );
}

export { Kbd, Keycaps };
