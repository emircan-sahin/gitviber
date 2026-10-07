import { ask } from "@/lib/app/ask";
import { Bell, CircleArrowUp, Code2, Gem, GitBranch, GitCompareArrows, Keyboard, Palette, RotateCcw, Sparkles, SquareArrowOutUpRight, SquareTerminal } from "lucide-react";
import { IS_MAC } from "@/lib/platform";
import { resetSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
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
export type Section = (typeof SECTIONS)[number]["id"];
const GROUPS = [...new Set(SECTIONS.map((s) => s.group))];

export const isSection = (id: unknown): id is Section => SECTIONS.some((s) => s.id === id);

/**
 * The nav and the open section, which the Settings dialog and the settings window both show.
 * `Title`: the nav's heading element; `actions`: buttons at the end of the header. `titleBar`: in
 * the settings window, whose title bar this is: dragged by it, and on macOS under its buttons.
 */
export function SettingsPanel({
  section,
  onSection,
  main,
  recording,
  setRecording,
  Title,
  actions,
  titleBar = false,
}: {
  section: Section;
  onSection: (s: Section) => void;
  /** The open project's main worktree, for its own settings; null with none open. */
  main: string | null;
  recording: Recording;
  setRecording: (r: Recording) => void;
  Title: React.ComponentType<{ className?: string; children: React.ReactNode }>;
  actions?: React.ReactNode;
  titleBar?: boolean;
}) {
  const current = SECTIONS.find((s) => s.id === section);
  const resetAll = async () => {
    const ok = await ask("Reset every setting, including keyboard shortcuts, to its default? Your own Open in apps and the repositories you sign off in stay.", { title: "Reset settings", kind: "warning", okLabel: "Reset" });
    if (ok) resetSettings();
  };
  return (
    <>
      {/* At the smallest window the sections and Reset all only just fit: past that, the list scrolls. */}
      <nav data-tauri-drag-region={titleBar || undefined} className={cn("flex w-48 shrink-0 flex-col gap-0.5 overflow-y-auto border-r border-border bg-sidebar p-2", titleBar && IS_MAC && "pt-8")}>
        <Title className="px-2 pt-1.5 pb-2.5">Settings</Title>
        {GROUPS.map((group) => (
          <div key={group} className="flex flex-col gap-0.5 not-first-of-type:mt-3">
            <GroupLabel className="px-2 pb-1">{group}</GroupLabel>
            {SECTIONS.filter((s) => s.group === group).map(({ id, label, icon: Icon }) => (
              <button
                key={id}
                onClick={() => onSection(id)}
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
        <div data-tauri-drag-region={titleBar || undefined} className="flex h-11 shrink-0 items-center gap-1 border-b border-border pr-2 pl-5">
          <span className="mr-auto text-[13.5px] font-semibold">{current?.label}</span>
          {actions}
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-5 pb-5">
          {section === "appearance" && <AppearanceSection />}
          {section === "editor" && <EditorSection />}
          {section === "diff" && <DiffSection />}
          {section === "terminal" && <TerminalSection />}
          {section === "obsidian" && <ObsidianSection />}
          {/* Keyed: the settings window stays open as the workspace opens another project. */}
          {section === "git" && <GitSection key={main} main={main} />}
          {section === "commit" && <CommitSection />}
          {section === "openIn" && <OpenInSection />}
          {section === "notifications" && <NotificationsSection />}
          {section === "shortcuts" && <ShortcutsSection recording={recording} setRecording={setRecording} />}
          {section === "updates" && <UpdatesSection />}
        </div>
      </div>
    </>
  );
}
