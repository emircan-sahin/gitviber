import { AppWindow, X } from "lucide-react";
import { useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Tip } from "@/components/ui/tooltip";
import { api } from "@/lib/api";
import { failed } from "@/lib/app/toast";
import { createStore } from "@/lib/store";
import type { Recording } from "./sections/Shortcuts";
import { type Section, SettingsPanel } from "./SettingsPanel";
import { useShareWithSettingsWindow } from "./SettingsWindow";

// Open state lives outside React so the top bar and ⌘, can open it from anywhere.
const openSection = createStore<Section | null>(null);
let lastSection: Section = "appearance";
function setOpen(s: Section | null) {
  if (s) lastSection = s;
  openSection.set(s);
}
/** Opens on the given section, else where the user left it: in the settings window while it's open, else here. */
export function openSettings(section?: Section) {
  const here = () => setOpen(section ?? lastSection);
  api.settingsWindow(section ?? null, false).then((shown) => shown || here(), here);
}

/** `main`: the open project's main worktree, for its own settings; null with none open. */
export function SettingsDialog({ main }: { main: string | null }) {
  const section = openSection.use();
  const [recording, setRecording] = useState<Recording>(null);
  const content = useRef<HTMLDivElement>(null);
  useShareWithSettingsWindow(main);

  const toWindow = () => {
    if (!section) return;
    setOpen(null);
    api.settingsWindow(section, true).catch(failed("Could not open the settings window"));
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
        <DialogDescription className="sr-only">Appearance, keyboard shortcut, Open in, notification, update, editor, diff, terminal, Obsidian, git and commit message preferences.</DialogDescription>
        {section && (
          <SettingsPanel
            section={section}
            onSection={setOpen}
            main={main}
            recording={recording}
            setRecording={setRecording}
            Title={DialogTitle}
            actions={
              <>
                <Tip label="Open in window">
                  <Button variant="ghost" size="icon" onClick={toWindow}>
                    <AppWindow />
                  </Button>
                </Tip>
                <DialogClose asChild>
                  <Button variant="ghost" size="icon" aria-label="Close">
                    <X />
                  </Button>
                </DialogClose>
              </>
            }
          />
        )}
      </DialogContent>
    </Dialog>
  );
}
