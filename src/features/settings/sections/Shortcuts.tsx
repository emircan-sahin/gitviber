import { Plus, RotateCcw, Search, TriangleAlert, X } from "lucide-react";
import { useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tip } from "@/components/ui/tooltip";
import { bindingsFor, COMMANDS, type Command, type CommandId, commandFor, eventChord, formatChord, isReserved } from "@/lib/commands/commands";
import { IS_MAC } from "@/lib/platform";
import { updateSettings, useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { Switch } from "@/components/ui/switch";
import { Keycaps } from "@/components/ui/kbd";
import { Field } from "@/features/settings/controls";

export type Recording = { id: CommandId; index: number } | null;

function setBinding(id: CommandId, keys: string[] | null, overrides: Record<string, string[]>) {
  const next = { ...overrides };
  const defaults = bindingsFor(id, {});
  // Storing what equals the default would pin it and hide future default changes.
  if (keys === null || (keys.length === defaults.length && keys.every((k, i) => k === defaults[i]))) delete next[id];
  else next[id] = keys;
  updateSettings({ keybindings: next });
}

/** Why a chord is ambiguous, naming which command actually runs. */
function clashNote(c: Command, chord: string, overrides: Record<string, string[]>): string | null {
  // By default some local commands take a global key over in their own place (⌘W in the terminal,
  // ⌘⌫ in the explorer): not a clash until the user rebinds one of the two.
  const meant = (o: Command) => ("local" in o) !== ("local" in c) && !(o.id in overrides) && !(c.id in overrides);
  const others = COMMANDS.filter((o) => o.id !== c.id && bindingsFor(o.id, overrides).includes(chord) && !meant(o));
  if (!others.length) return null;
  const k = formatChord(chord);
  const global = others.filter((o) => !("local" in o));
  // Two local commands never meet: each listens in its own place.
  if ("local" in c) return global.length ? `${k} also runs ${global.map((o) => o.title).join(", ")} outside ${c.local}` : null;
  if (!global.length) return `${k} also runs ${others.map((o) => ("local" in o ? `${o.title} in ${o.local}` : o.title)).join(", ")}`;
  const winner = commandFor(chord, overrides);
  return winner?.id === c.id ? `${k} is also bound to ${global.map((o) => o.title).join(", ")}; this command takes precedence` : `${k} is also bound to ${winner?.title}, which takes precedence`;
}

export function ShortcutsSection({ recording, setRecording }: { recording: Recording; setRecording: (r: Recording) => void }) {
  const { keybindings, shortcutOverlay } = useSettings();
  const [query, setQuery] = useState("");

  const q = query.trim().toLowerCase();
  const rows = COMMANDS.filter((c) => {
    if (!q) return true;
    const keys = bindingsFor(c.id, keybindings);
    return [c.title, c.category, c.id, ...keys, ...keys.map(formatChord)].some((t) => t.toLowerCase().includes(q));
  });

  return (
    <>
      <Field
        label={`Hold ${IS_MAC ? "⌘" : "Ctrl"} to show shortcuts`}
        hint="Hold it by itself for a second to see the shortcuts that work where you are. The key below shows them too."
        commands={["workbench.shortcutOverlay"]}
      >
        <Switch checked={shortcutOverlay} onChange={(v) => updateSettings({ shortcutOverlay: v })} />
      </Field>
      <div className="sticky top-0 z-10 -mx-5 flex items-center gap-2 bg-elevated px-5 pt-4 pb-3">
        <div className="relative flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-subtle" />
          <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={`Search commands or keys (e.g. ${formatChord("cmd+b")})`} className="pl-8" />
        </div>
        <Button variant="secondary" disabled={!Object.keys(keybindings).length} onClick={() => updateSettings({ keybindings: {} })}>
          <RotateCcw /> Reset all
        </Button>
      </div>
      <div className="overflow-hidden rounded-md border border-border">
        {rows.map((c) => {
          const keys = bindingsFor(c.id, keybindings);
          const clashes = keys.map((k) => clashNote(c, k, keybindings)).filter(Boolean);
          const record = (index: number) => ({
            active: recording?.id === c.id && recording.index === index,
            onStart: () => setRecording({ id: c.id, index }),
            onStop: () => setRecording(null),
            // Replaces the clicked key only (or adds one); duplicates collapse.
            onRecord: (chord: string) => setBinding(c.id, [...new Set([...keys.slice(0, index), chord, ...keys.slice(index + 1)])], keybindings),
          });
          const adding = record(keys.length);
          return (
            <div key={c.id} className="group flex min-h-9 items-center gap-3 border-b border-border px-3 py-1 last:border-0 hover:bg-hover/50 focus-visible:bg-hover/50">
              <div className="min-w-0 flex-1">
                <div className="truncate text-[12.5px]">{c.title}</div>
                <div className="truncate text-[11px] text-subtle">{c.category}</div>
              </div>
              {clashes.length > 0 && (
                <Tip label={clashes.join(". ")}>
                  <span tabIndex={0} aria-label={clashes.join(". ")} className="shrink-0 rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-ring">
                    <TriangleAlert className="size-3.5 text-modified" />
                  </span>
                </Tip>
              )}
              <div className="flex shrink-0 items-center gap-1">
                {keys.map((k, i) => (
                  <Recorder key={k} label={<Keycaps chord={k} />} title="Change this key" {...record(i)} />
                ))}
                <Recorder
                  label={keys.length ? <Plus className="size-3.5" /> : <span className="text-[11.5px] text-subtle">Unbound</span>}
                  title="Add a key"
                  className={cn(keys.length > 0 && !adding.active && "text-subtle opacity-0 group-hover:opacity-100 focus-visible:opacity-100")}
                  {...adding}
                />
              </div>
              <div className="flex w-12 shrink-0 justify-end gap-0.5">
                {c.id in keybindings && (
                  <Tip label="Reset to default">
                    <Button variant="ghost" size="icon-sm" onClick={() => setBinding(c.id, null, keybindings)}>
                      <RotateCcw />
                    </Button>
                  </Tip>
                )}
                {keys.length > 0 && (
                  <Tip label="Remove all keys">
                    <Button variant="ghost" size="icon-sm" className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100" onClick={() => setBinding(c.id, [], keybindings)}>
                      <X />
                    </Button>
                  </Tip>
                )}
              </div>
            </div>
          );
        })}
        {!rows.length && <div className="px-3 py-6 text-center text-[12px] text-subtle">No matching commands</div>}
      </div>
      <p className="mt-3 text-[11.5px] leading-relaxed text-subtle">
        Click a key to change it, or + to add one; Esc cancels. When two commands share a key, the one higher in this list runs. While you type in a text field, only
        shortcuts with {IS_MAC ? "⌘, ⌃ or an F-key apply (not ⌘-arrows or ⌃ with a letter, which edit text)" : "Ctrl or an F-key apply (not Ctrl+arrows, which move by word)"}, and Commit only applies in
        the commit message.
      </p>
    </>
  );
}

function Recorder({
  label,
  title,
  className,
  active,
  onStart,
  onStop,
  onRecord,
}: {
  label: React.ReactNode;
  title: string;
  className?: string;
  active: boolean;
  onStart: () => void;
  onStop: () => void;
  onRecord: (chord: string) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [refused, setRefused] = useState<string | null>(null);
  const start = () => {
    setRefused(null);
    // WebKit doesn't focus on click, and keys only reach a focused element.
    ref.current?.focus();
    onStart();
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!active) {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        start();
      }
      return;
    }
    const chord = eventChord(e.nativeEvent);
    // Tab still moves focus (and blurring cancels), so it can't lock the user in.
    if (chord === "tab" || chord === "shift+tab") return;
    // Handled here: the global shortcut handler skips prevented events.
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") return onStop();
    if (!chord) return;
    if (isReserved(chord)) return setRefused(`${formatChord(chord)} is reserved by macOS`);
    onRecord(chord);
    onStop();
  };
  // A div, not a button: a button would turn the Space keyup after recording into a click
  // that starts recording again.
  return (
    <div
      ref={ref}
      role="button"
      tabIndex={0}
      aria-label={title}
      onClick={() => (active ? onStop() : start())}
      onBlur={() => active && onStop()}
      onKeyDown={onKeyDown}
      className={cn(
        "flex h-7 min-w-7 cursor-pointer items-center justify-center rounded-md px-1 outline-none select-none focus-visible:ring-1 focus-visible:ring-ring",
        active ? "ring-1 ring-primary" : "hover:bg-active focus-visible:bg-active",
        className,
      )}
    >
      {active ? <span className={cn("px-1 text-[11.5px]", refused ? "text-destructive" : "text-primary")}>{refused ?? "Press keys…"}</span> : label}
    </div>
  );
}
