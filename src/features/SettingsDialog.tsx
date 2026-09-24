import { ask } from "@tauri-apps/plugin-dialog";
import { Code2, GitBranch, GitCompareArrows, Keyboard, Palette, Plus, RotateCcw, Search, Sparkles, SquareArrowOutUpRight, TriangleAlert, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Tip } from "@/components/ui/tooltip";
import type { Whitespace } from "@/lib/api";
import { bindingsFor, COMMANDS, type Command, type CommandId, commandFor, eventChord, formatChord, IS_MAC, RESERVED } from "@/lib/commands";
import { refreshOpenApps, useOpenApps } from "@/lib/openIn";
import {
  type Appearance,
  type CodeFont,
  type CustomApp,
  cleanFontName,
  codeFontChoices,
  codeFontFamily,
  DEFAULT_FONT_SIZE,
  FETCH_INTERVALS,
  LIGHT_SYNTAX_THEMES,
  type LightSyntaxTheme,
  resetSettings,
  type Settings,
  SYNTAX_THEMES,
  type SyntaxTheme,
  UI_FONTS,
  UI_SCALES,
  type UiFont,
  updateSettings,
  useSettings,
} from "@/lib/settings";
import { SUGGEST_LIMIT_KB, SUGGEST_PRESETS, SUGGEST_PROMPT } from "@/lib/suggest";
import { cn } from "@/lib/utils";

const SECTIONS = [
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "editor", label: "Editor", icon: Code2 },
  { id: "diff", label: "Diff", icon: GitCompareArrows },
  { id: "git", label: "Git", icon: GitBranch },
  { id: "commit", label: "Commit Messages", icon: Sparkles },
  { id: "openIn", label: "Open In", icon: SquareArrowOutUpRight },
  { id: "shortcuts", label: "Keyboard Shortcuts", icon: Keyboard },
] as const;
type Section = (typeof SECTIONS)[number]["id"];

// Open state lives outside React so the top bar and ⌘, can open it from anywhere.
let openSection: Section | null = null;
let lastSection: Section = "appearance";
const listeners = new Set<() => void>();
function setOpen(s: Section | null) {
  openSection = s;
  if (s) lastSection = s;
  listeners.forEach((l) => l());
}
/** Opens on the given section, else where the user left it. */
export function openSettings(section: Section = lastSection) {
  setOpen(section);
}

export function SettingsDialog() {
  const section = useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => openSection,
  );
  const [recording, setRecording] = useState<Recording>(null);
  const content = useRef<HTMLDivElement>(null);
  const current = SECTIONS.find((s) => s.id === section);

  const resetAll = async () => {
    const ok = await ask("Reset every setting, including keyboard shortcuts, to its default? Your own Open in apps and the repositories you sign off in stay.", { title: "Reset settings", kind: "warning", okLabel: "Reset" });
    if (ok) resetSettings();
  };

  return (
    <Dialog open={!!section} onOpenChange={(o) => !o && setOpen(null)}>
      <DialogContent
        // Escape cancels a shortcut recording rather than closing the window.
        onEscapeKeyDown={(e) => recording && e.preventDefault()}
        // Take focus off the workspace (its lists and tree react to keys) without ringing
        // the first nav item, as the default auto-focus would.
        ref={content}
        tabIndex={-1}
        onOpenAutoFocus={(e) => {
          e.preventDefault();
          content.current?.focus();
        }}
        className="top-1/2 flex outline-none h-[min(620px,calc(100vh-64px))] w-[calc(100vw-48px)] max-w-[880px] -translate-y-1/2 overflow-hidden p-0"
      >
        <nav className="flex w-48 shrink-0 flex-col gap-0.5 border-r border-border bg-sidebar p-2">
          <DialogTitle className="px-2 pt-1.5 pb-2.5">Settings</DialogTitle>
          <DialogDescription className="sr-only">Appearance, editor, diff, git, commit message, Open in and keyboard shortcut preferences.</DialogDescription>
          {SECTIONS.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              onClick={() => setOpen(id)}
              className={cn(
                "flex h-7 items-center gap-2 rounded-md px-2 text-left text-[12.5px]",
                section === id ? "bg-active text-foreground" : "text-muted-foreground hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground",
              )}
            >
              <Icon className="size-3.5 shrink-0" /> {label}
            </button>
          ))}
          <button onClick={resetAll} className="mt-auto flex h-7 items-center gap-2 rounded-md px-2 text-left text-[12px] text-muted-foreground hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground">
            <RotateCcw className="size-3.5 shrink-0" /> Reset all settings
          </button>
        </nav>
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex h-11 shrink-0 items-center border-b border-border pr-2 pl-5">
            <span className="text-[13.5px] font-semibold">{current?.label}</span>
            <DialogClose asChild>
              <Button variant="ghost" size="icon" className="ml-auto">
                <X />
              </Button>
            </DialogClose>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-5 pb-5">
            {section === "appearance" && <AppearanceSection />}
            {section === "editor" && <EditorSection />}
            {section === "diff" && <DiffSection />}
            {section === "git" && <GitSection />}
            {section === "commit" && <CommitSection />}
            {section === "openIn" && <OpenInSection />}
            {section === "shortcuts" && <ShortcutsSection recording={recording} setRecording={setRecording} />}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function AppearanceSection() {
  const s = useSettings();
  return (
    <>
      <Field label="Theme" hint="System follows macOS. Dimmed is a softer, lighter dark.">
        <Segmented<Appearance>
          value={s.appearance}
          onChange={(v) => updateSettings({ appearance: v })}
          options={[
            ["system", "System"],
            ["light", "Light"],
            ["dark", "Dark"],
            ["dim", "Dimmed"],
          ]}
        />
      </Field>
      {s.appearance === "system" && (
        <Field label="Dark variant" hint="The dark theme System uses while macOS is dark.">
          <Segmented<Settings["darkVariant"]>
            value={s.darkVariant}
            onChange={(v) => updateSettings({ darkVariant: v })}
            options={[
              ["dark", "Dark"],
              ["dim", "Dimmed"],
            ]}
          />
        </Field>
      )}
      {/* Each appearance keeps its own syntax theme, so switching back restores it. */}
      <Field label="Dark syntax theme" hint="Code colors while the app is dark.">
        <Select value={s.syntaxTheme} options={SYNTAX_THEMES} onChange={(v) => updateSettings({ syntaxTheme: v as SyntaxTheme })} />
      </Field>
      <Field label="Light syntax theme" hint="Code colors while the app is light.">
        <Select value={s.lightSyntaxTheme} options={LIGHT_SYNTAX_THEMES} onChange={(v) => updateSettings({ lightSyntaxTheme: v as LightSyntaxTheme })} />
      </Field>
      <Field label="Interface font" hint="The code font is under Editor.">
        <FontPicker
          fonts={Object.keys(UI_FONTS)}
          value={s.uiFont}
          custom={s.customUiFont}
          onChange={(uiFont, customUiFont) => updateSettings({ uiFont: uiFont as UiFont, customUiFont })}
        />
      </Field>
      <Field label="Interface scale" hint="Zooms the whole window. The code font size stays its own setting." commands={["view.zoomIn", "view.zoomOut", "view.zoomReset"]}>
        <Segmented<string>
          value={String(s.uiScale)}
          onChange={(v) => updateSettings({ uiScale: Number(v) })}
          options={UI_SCALES.map((z) => [String(z), `${Math.round(z * 100)}%`])}
        />
      </Field>
    </>
  );
}

const SAMPLE = `const total = items.filter((x) => x.price >= 10)
  .reduce((sum, x) => sum + x.price, 0); // != === =>`;

function EditorSection() {
  const s = useSettings();
  return (
    <>
      <pre
        className="mt-4 overflow-hidden rounded-md border border-border bg-background px-3 py-2 whitespace-pre text-muted-foreground"
        style={{
          fontFamily: codeFontFamily(s),
          fontSize: s.codeFontSize,
          lineHeight: `${Math.round(s.codeFontSize * s.lineHeight)}px`,
          fontVariantLigatures: s.ligatures ? "normal" : "none",
        }}
      >
        {SAMPLE}
      </pre>
      <Field label="Font">
        <FontPicker
          fonts={codeFontChoices}
          value={s.codeFont}
          custom={s.customCodeFont}
          onChange={(codeFont, customCodeFont) => updateSettings({ codeFont: codeFont as CodeFont, customCodeFont })}
        />
      </Field>
      <Field label="Font size" commands={["editor.fontZoomIn", "editor.fontZoomOut", "editor.fontZoomReset"]}>
        <div className="flex items-center gap-1">
          <Button variant="secondary" size="icon-sm" onClick={() => updateSettings({ codeFontSize: s.codeFontSize - 0.5 })}>
            −
          </Button>
          <span className="w-10 text-center font-mono text-[12px]">{s.codeFontSize}</span>
          <Button variant="secondary" size="icon-sm" onClick={() => updateSettings({ codeFontSize: s.codeFontSize + 0.5 })}>
            +
          </Button>
          <Tip label="Reset">
            <Button variant="ghost" size="icon-sm" disabled={s.codeFontSize === DEFAULT_FONT_SIZE} onClick={() => updateSettings({ codeFontSize: DEFAULT_FONT_SIZE })}>
              <RotateCcw />
            </Button>
          </Tip>
        </div>
      </Field>
      <Field label="Line height">
        <Segmented<string>
          value={String(s.lineHeight)}
          onChange={(v) => updateSettings({ lineHeight: Number(v) })}
          options={[
            ["1.4", "Compact"],
            ["1.6", "Comfortable"],
            ["1.8", "Relaxed"],
          ]}
        />
      </Field>
      <Field label="Font ligatures" hint="Joins pairs like => and != in fonts that have them (JetBrains Mono).">
        <Switch checked={s.ligatures} onChange={(v) => updateSettings({ ligatures: v })} />
      </Field>
      <Field label="Word wrap" hint="Wrap long lines instead of scrolling sideways." commands={["editor.toggleWrap"]}>
        <Switch checked={s.wordWrap} onChange={(v) => updateSettings({ wordWrap: v })} />
      </Field>
      <Field label="Open Markdown as preview" hint="Opening a .md file shows it rendered. Diffs of Markdown files always start on the diff.">
        <Switch checked={s.markdownPreview} onChange={(v) => updateSettings({ markdownPreview: v })} />
      </Field>
    </>
  );
}

function DiffSection() {
  const s = useSettings();
  return (
    <>
      <Field label="Layout" hint="Unified shows one column, split shows before and after side by side." commands={["diff.toggleSplit"]}>
        <Segmented<string>
          value={s.sideBySide ? "split" : "unified"}
          onChange={(v) => updateSettings({ sideBySide: v === "split" })}
          options={[
            ["unified", "Unified"],
            ["split", "Split"],
          ]}
        />
      </Field>
      <Field label="Collapse unchanged lines" hint="Fold long runs of unchanged code between changes." commands={["diff.toggleCollapse"]}>
        <Switch checked={s.hideUnchanged} onChange={(v) => updateSettings({ hideUnchanged: v })} />
      </Field>
      <Field
        label="Ignore whitespace"
        hint="Changes hides re-indented lines and trailing spaces, like git diff -b. All ignores every space, like git diff -w."
        commands={["diff.toggleWhitespace"]}
      >
        <Segmented<string>
          value={s.ignoreWhitespace ? s.whitespaceMode : "off"}
          onChange={(v) => updateSettings(v === "off" ? { ignoreWhitespace: false } : { ignoreWhitespace: true, whitespaceMode: v as Whitespace })}
          options={[
            ["off", "Off"],
            ["amount", "Changes"],
            ["all", "All"],
          ]}
        />
      </Field>
    </>
  );
}

function GitSection() {
  const s = useSettings();
  return (
    <Field label="Fetch in the background" hint="Keeps ahead / behind and the remote branches current for the open repository. A fetch that fails, say while offline, stays quiet.">
      <Segmented<string>
        value={String(s.backgroundFetch)}
        onChange={(v) => updateSettings({ backgroundFetch: Number(v) })}
        options={FETCH_INTERVALS.map((m) => [String(m), m ? `${m} min` : "Off"])}
      />
    </Field>
  );
}

type Preset = keyof typeof SUGGEST_PRESETS;

function CommitSection() {
  const s = useSettings();
  const preset = (Object.keys(SUGGEST_PRESETS) as Preset[]).find((k) => SUGGEST_PRESETS[k].command === s.suggestCommand.trim());
  // Custom stays picked while its text happens to match a preset.
  const [custom, setCustom] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  return (
    <>
      <Field
        label="Suggest commit messages"
        hint="Adds a ✦ button to the commit box that asks your own agent CLI to write the message. GitViber sends nothing itself and keeps no keys: the command runs on this Mac, and it decides where the diff goes."
      >
        <Switch checked={s.suggestEnabled} onChange={(v) => updateSettings({ suggestEnabled: v })} />
      </Field>
      <Field
        label="Command"
        hint="Runs in the repository's folder, directly, not through a shell. The prompt and the diff arrive on stdin; put {prompt} in the command to pass the prompt as an argument instead. If it isn't found, give its full path (`which claude` in Terminal prints it)."
        commands={["git.suggestMessage"]}
      >
        <div className="flex w-64 flex-col gap-2">
          <Segmented<Preset | "custom">
            value={custom || !preset ? "custom" : preset}
            onChange={(v) => {
              setCustom(v === "custom");
              if (v === "custom") input.current?.focus();
              else updateSettings({ suggestCommand: SUGGEST_PRESETS[v].command });
            }}
            options={[...(Object.keys(SUGGEST_PRESETS) as Preset[]).map((k): [Preset, string] => [k, SUGGEST_PRESETS[k].label]), ["custom", "Custom"]]}
          />
          <Input ref={input} value={s.suggestCommand} onChange={(e) => updateSettings({ suggestCommand: e.target.value })} placeholder="claude -p" spellCheck={false} className="font-mono" />
        </div>
      </Field>
      <div className="py-3.5">
        <div className="text-[12.5px] font-medium">What the command gets</div>
        <div className="mt-0.5 text-[11.5px] leading-relaxed text-muted-foreground">Only when you click ✦, and nothing else from the app:</div>
        <pre className="mt-2 rounded-md border border-border bg-background px-3 py-2 font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap text-muted-foreground">
          {SUGGEST_PROMPT}
          {"\n\n"}
          <span className="text-subtle">
            [the diff: the staged changes, or every change when nothing is staged (Commit all), or the whole commit when amending; up to {SUGGEST_LIMIT_KB} KB, with a note in the
            prompt when it's cut]
          </span>
        </pre>
      </div>
    </>
  );
}

function OpenInSection() {
  const s = useSettings();
  const { apps } = useOpenApps();
  useEffect(refreshOpenApps, []);
  const setCustom = (list: CustomApp[]) => updateSettings({ openInCustom: list });
  return (
    <>
      <Field label="Default app" hint="What a click on Open in (in the status bar) runs. Picking an app from its list makes that one the default." commands={["file.openIn"]}>
        <Select value={s.openInApp} options={{ "": "None: show the list", ...Object.fromEntries(apps.map((a) => [a.id, a.name])) }} onChange={(v) => updateSettings({ openInApp: v })} />
      </Field>
      <Field label="Show detected apps" hint="Editors, terminals and git clients found on this Mac. Turn off to list only your own apps.">
        <Switch checked={!s.openInHideBuiltins} onChange={(v) => updateSettings({ openInHideBuiltins: !v })} />
      </Field>
      <div className="py-3.5">
        <div className="text-[12.5px] font-medium">Your apps</div>
        <div className="mt-0.5 text-[11.5px] leading-relaxed text-muted-foreground">
          A command run directly, not through a shell. <Kbd>{"{path}"}</Kbd> is the worktree, <Kbd>{"{file}"}</Kbd> the open file (the worktree when none is), <Kbd>{"{line}"}</Kbd> the line in
          view. With none of them, the file or folder goes last. Example: <Kbd>{"code -g {file}:{line}"}</Kbd>
        </div>
        <div className="mt-3 flex flex-col gap-1.5">
          {s.openInCustom.map((app, i) => (
            <CustomAppRow
              key={app.id}
              app={app}
              onChange={(next) => setCustom(s.openInCustom.map((c, j) => (j === i ? next : c)))}
              onRemove={() => setCustom(s.openInCustom.filter((_, j) => j !== i))}
            />
          ))}
        </div>
        <Button variant="secondary" size="sm" className="mt-2" onClick={() => setCustom([...s.openInCustom, { id: `custom:${Date.now().toString(36)}`, name: "", command: "" }])}>
          <Plus /> Add app
        </Button>
      </div>
    </>
  );
}

/** Saved on leaving a field: every keystroke would re-render everything that reads settings. */
function CustomAppRow({ app, onChange, onRemove }: { app: CustomApp; onChange: (a: CustomApp) => void; onRemove: () => void }) {
  const [draft, setDraft] = useState(app);
  const commit = () => (draft.name !== app.name || draft.command !== app.command) && onChange(draft);
  const field = (key: "name" | "command") => ({
    value: draft[key],
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => setDraft({ ...draft, [key]: e.target.value }),
    onBlur: commit,
    onKeyDown: (e: React.KeyboardEvent) => e.key === "Enter" && commit(),
  });
  return (
    <div className="flex items-center gap-1.5">
      <Input {...field("name")} placeholder="Name" className="w-36" />
      <Input {...field("command")} placeholder="nvim-qt {file}" className="flex-1 font-mono" spellCheck={false} />
      <Tip label="Remove">
        <Button variant="ghost" size="icon-sm" onClick={onRemove}>
          <X />
        </Button>
      </Tip>
    </div>
  );
}

type Recording = { id: CommandId; index: number } | null;

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
  const others = COMMANDS.filter((o) => o.id !== c.id && bindingsFor(o.id, overrides).includes(chord));
  if (!others.length) return null;
  const k = formatChord(chord);
  const global = others.filter((o) => !("local" in o));
  // Two local commands never meet: each listens in its own place.
  if ("local" in c) return global.length ? `${k} also runs ${global.map((o) => o.title).join(", ")} outside ${c.local}` : null;
  if (!global.length) return `${k} also runs ${others.map((o) => ("local" in o ? `${o.title} in ${o.local}` : o.title)).join(", ")}`;
  const winner = commandFor(chord, overrides);
  return winner?.id === c.id ? `${k} is also bound to ${global.map((o) => o.title).join(", ")}; this command takes precedence` : `${k} is also bound to ${winner?.title}, which takes precedence`;
}

function ShortcutsSection({ recording, setRecording }: { recording: Recording; setRecording: (r: Recording) => void }) {
  const { keybindings } = useSettings();
  const [query, setQuery] = useState("");

  const q = query.trim().toLowerCase();
  const rows = COMMANDS.filter((c) => {
    if (!q) return true;
    const keys = bindingsFor(c.id, keybindings);
    return [c.title, c.category, c.id, ...keys, ...keys.map(formatChord)].some((t) => t.toLowerCase().includes(q));
  });

  return (
    <>
      <div className="sticky top-0 z-10 -mx-5 flex items-center gap-2 bg-elevated px-5 pt-4 pb-3">
        <div className="relative flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-subtle" />
          <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search commands or keys (e.g. ⌘B)" className="pl-8" />
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
                  <Recorder key={k} label={<Kbd>{formatChord(k)}</Kbd>} title="Change this key" {...record(i)} />
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
    if (RESERVED.includes(chord)) return setRefused(`${formatChord(chord)} is reserved by macOS`);
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

function Kbd({ children }: { children: React.ReactNode }) {
  return <kbd className="rounded-sm border border-border-strong bg-background px-1.5 py-px font-mono text-[11px] text-foreground">{children}</kbd>;
}

function Field({ label, hint, commands, children }: { label: string; hint?: string; commands?: CommandId[]; children: React.ReactNode }) {
  const { keybindings } = useSettings();
  const keys = commands?.map((id) => bindingsFor(id, keybindings)[0]).filter(Boolean) ?? [];
  return (
    <div className="flex flex-wrap items-center gap-x-6 gap-y-2 border-b border-border py-3.5 last:border-0">
      <div className="min-w-48 flex-1">
        <div className="text-[12.5px] font-medium">{label}</div>
        {hint && <div className="mt-0.5 text-[11.5px] leading-relaxed text-muted-foreground">{hint}</div>}
        {keys.length > 0 && (
          <div className="mt-1.5 flex gap-1">
            {keys.map((k) => (
              <Kbd key={k}>{formatChord(k)}</Kbd>
            ))}
          </div>
        )}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function Segmented<T extends string>({ value, onChange, options }: { value: T; onChange: (v: T) => void; options: [T, string][] }) {
  return (
    <div className="flex h-7 overflow-hidden rounded-md border border-border-strong">
      {options.map(([v, label]) => (
        <button
          key={v}
          onClick={() => onChange(v)}
          className={cn(
            "border-r border-border-strong px-2.5 text-[12px] last:border-r-0",
            v === value ? "bg-primary/15 text-primary" : "text-muted-foreground hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground",
          )}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

function Select({ value, options, onChange }: { value: string; options: Record<string, string>; onChange: (v: string) => void }) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className="h-7 w-52 rounded-md border border-border-strong bg-background px-2 text-[12px] text-foreground outline-none focus:border-primary"
    >
      {Object.entries(options).map(([id, label]) => (
        <option key={id} value={id}>
          {label}
        </option>
      ))}
    </select>
  );
}

/** A preset font, or Custom with a field for any installed font's name. */
function FontPicker({ fonts, value, custom, onChange }: { fonts: string[]; value: string; custom: string; onChange: (font: string, custom: string) => void }) {
  const [draft, setDraft] = useState(custom);
  const name = cleanFontName(draft);
  const missing = useMemo(() => !!name && !fontInstalled(name), [name]);
  // Applied on Enter or leaving the field: every partial name on the way would re-lay out the code view.
  const commit = () => name !== custom && onChange("Custom", name);
  return (
    <div className="flex w-52 flex-col gap-1.5">
      <Select value={value} options={Object.fromEntries([...fonts, "Custom"].map((f) => [f, f]))} onChange={(v) => onChange(v, custom)} />
      {value === "Custom" && (
        <>
          <Input value={draft} placeholder="Installed font name" onChange={(e) => setDraft(e.target.value)} onBlur={commit} onKeyDown={(e) => e.key === "Enter" && commit()} />
          {missing && <div className="text-[11px] leading-snug text-modified">Not installed: the default font shows instead.</div>}
        </>
      )}
    </div>
  );
}

/** Whether a font is installed: text set in it measures differently from every generic fallback. */
function fontInstalled(name: string) {
  const ctx = document.createElement("canvas").getContext("2d")!;
  const width = (font: string) => {
    ctx.font = `40px ${font}`;
    return ctx.measureText("mmmwwwiiilll0O@").width;
  };
  return ["monospace", "serif", "sans-serif"].some((generic) => width(`"${name}", ${generic}`) !== width(generic));
}

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
