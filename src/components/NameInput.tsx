import { useLayoutEffect, useRef } from "react";

/** Inline name editor: Enter or blur commits, Escape cancels (VS Code behavior). */
export function NameInput({ initial, selectStem, onDone }: { initial: string; selectStem?: boolean; onDone: (name: string | null, refocus: boolean) => void }) {
  const done = useRef(false);
  // Unmounted by its owner (the row or tab went): a blur fired during removal must not commit.
  // Reset on mount too: StrictMode's mount/unmount/mount would otherwise leave it true, and the
  // input then ignored Enter, Escape and blur.
  useLayoutEffect(() => {
    done.current = false;
    return () => {
      done.current = true;
    };
  }, []);
  const finish = (name: string | null, refocus: boolean) => {
    if (done.current) return;
    done.current = true;
    onDone(name, refocus);
  };
  return (
    <input
      autoFocus
      defaultValue={initial}
      spellCheck={false}
      autoCapitalize="off"
      autoCorrect="off"
      onFocus={(ev) => {
        // Select "name" of "name.ext" so typing keeps the extension.
        const dot = initial.lastIndexOf(".");
        ev.currentTarget.setSelectionRange(0, selectStem && dot > 0 ? dot : initial.length);
      }}
      onClick={(ev) => ev.stopPropagation()}
      onKeyDown={(ev) => {
        ev.stopPropagation();
        // Enter that confirms an IME composition (e.g. Japanese input) isn't a commit.
        if (ev.key === "Enter" && !ev.nativeEvent.isComposing && ev.keyCode !== 229) finish(ev.currentTarget.value, true);
        else if (ev.key === "Escape") finish(null, true);
      }}
      onBlur={(ev) => finish(ev.currentTarget.value, false)}
      className="h-5 min-w-0 flex-1 rounded-sm border border-primary bg-background px-1 text-[12px] text-foreground outline-none select-text"
    />
  );
}
