import { ask } from "@/lib/app/ask";
import { Bell, CircleArrowUp, Code2, Gem, GitBranch, GitCompareArrows, Keyboard, Palette, RotateCcw, Sparkles, SquareArrowOutUpRight, SquareTerminal, X } from "lucide-react";
import { useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { resetSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { createStore } from "@/lib/store";
import { GroupLabel } from "./controls";
import { AppearanceSection } from "./sections/Appearance";
import { EditorSection } from "./sections/Editor";
import { DiffSection } from "./sections/Diff";
import { GitSection } from "./sections/Git";
import { CommitSection } from "./sections/Commit";
import { OpenInSection } from "./sections/OpenIn";
import { NotificationsSection } from "./sections/Notifications";
import { type Recording, ShortcutsSection } from "./sections/Shortcuts";
import { UpdatesSection } from "./sections/Updates";
import { TerminalSection } from "./sections/Terminal";
import { ObsidianSection } from "./sections/Obsidian";

const SECTIONS = [
  { id: "appearance", label: "Appearance", icon: Palette, group: "General" },
  { id: "shortcuts", label: "Keyboard Shortcuts", icon: Keyboard, group: "General" },
  { id: "openIn", label: "Open In", icon: SquareArrowOutUpRight, group: "General" },
  { id: "notifications", label: "Notifications", icon: Bell, group: "General" },
  { id: "updates", label: "Updates", icon: CircleArrowUp, group: "General" },
  { id: "editor", label: "Editor", icon: Code2, group: "Workspace" },
  { id: "diff", label: "Diff", icon: GitCompareArrows, group: "Workspace" },
  { id: "terminal", label: "Terminal", icon: SquareTerminal, group: "Workspace" },
  { id: "obsidian", label: "Obsidian", icon: Gem, group: "Workspace" },
  { id: "git", label: "Git", icon: GitBranch, group: "Git" },
  { id: "commit", label: "Commit Messages", icon: Sparkles, group: "Git" },
] as const;
type Section = (typeof SECTIONS)[number]["id"];
const GROUPS = [...new Set(SECTIONS.map((s) => s.group))];

// Open state lives outside React so the top bar and ⌘, can open it from anywhere.
const openSection = createStore<Section | null>(null);
let lastSection: Section = "appearance";
function setOpen(s: Section | null) {
  if (s) lastSection = s;
  openSection.set(s);
}
/** Opens on the given section, else where the user left it. */
export function openSettings(section: Section = lastSection) {
  setOpen(section);
}

/** `main`: the open project's main worktree, for its own settings; null with none open. */
export function SettingsDialog({ main }: { main: string | null }) {
  const section = openSection.use();
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
        {/* At the smallest window the sections and Reset all only just fit: past that, the list scrolls. */}
        <nav className="flex w-48 shrink-0 flex-col gap-0.5 overflow-y-auto border-r border-border bg-sidebar p-2">
          <DialogTitle className="px-2 pt-1.5 pb-2.5">Settings</DialogTitle>
          <DialogDescription className="sr-only">Appearance, keyboard shortcut, Open in, notification, update, editor, diff, terminal, Obsidian, git and commit message preferences.</DialogDescription>
          {GROUPS.map((group) => (
            <div key={group} className="flex flex-col gap-0.5 not-first-of-type:mt-3">
              <GroupLabel className="px-2 pb-1">{group}</GroupLabel>
              {SECTIONS.filter((s) => s.group === group).map(({ id, label, icon: Icon }) => (
                <button
                  key={id}
                  onClick={() => setOpen(id)}
                  aria-current={section === id ? "page" : undefined}
                  className={cn(
                    "flex h-7 items-center gap-2 rounded-md px-2 text-left text-[12.5px]",
                    section === id ? "bg-active text-foreground" : "text-muted-foreground hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground",
                  )}
                >
                  <Icon className="size-3.5 shrink-0" /> {label}
                </button>
              ))}
            </div>
          ))}
          <button onClick={resetAll} className="mt-auto flex h-7 items-center gap-2 rounded-md px-2 text-left text-[12px] text-muted-foreground hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground">
            <RotateCcw className="size-3.5 shrink-0" /> Reset all settings
          </button>
        </nav>
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex h-11 shrink-0 items-center border-b border-border pr-2 pl-5">
            <span className="text-[13.5px] font-semibold">{current?.label}</span>
            <DialogClose asChild>
              <Button variant="ghost" size="icon" className="ml-auto" aria-label="Close">
                <X />
              </Button>
            </DialogClose>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-5 pb-5">
            {section === "appearance" && <AppearanceSection />}
            {section === "editor" && <EditorSection />}
            {section === "diff" && <DiffSection />}
            {section === "terminal" && <TerminalSection />}
            {section === "obsidian" && <ObsidianSection />}
            {section === "git" && <GitSection main={main} />}
            {section === "commit" && <CommitSection />}
            {section === "openIn" && <OpenInSection />}
            {section === "notifications" && <NotificationsSection />}
            {section === "shortcuts" && <ShortcutsSection recording={recording} setRecording={setRecording} />}
            {section === "updates" && <UpdatesSection />}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
