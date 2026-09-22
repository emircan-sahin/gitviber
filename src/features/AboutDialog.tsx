import { listen } from "@tauri-apps/api/event";
import { Bug, Copy, ExternalLink, Scale } from "lucide-react";
import { useEffect, useState, useSyncExternalStore } from "react";
import { Logo } from "@/components/Logo";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { type About, api, errorMessage, github } from "@/lib/api";
import { toast } from "@/lib/toast";

const REPO = "https://github.com/emircan-sahin/gitviber";

let aboutOnce: Promise<About> | null = null;

/** Version, commit, OS and git; asked once per session. */
export function useAbout() {
  const [about, setAbout] = useState<About | null>(null);
  useEffect(() => {
    aboutOnce ??= api.about();
    aboutOnce.then(setAbout, () => {});
  }, []);
  return about;
}

/** What the bug report template asks for, in one line. */
export function versionLine(a: About) {
  const build = a.commit ? ` (${a.commit})` : "";
  return `GitViber ${a.version}${build} · ${a.os} ${a.arch} · git ${a.git ?? "not found"}`;
}

let open = false;
const listeners = new Set<() => void>();
function setOpen(o: boolean) {
  open = o;
  listeners.forEach((l) => l());
}
export const openAbout = () => setOpen(true);

const openLink = (url: string) => github.openUrl(url).catch((e) => toast("error", "Could not open the link", errorMessage(e)));

/** GitViber → About GitViber (lib.rs `menu` emits "show-about"), and the version in the status bar. */
export function AboutDialog() {
  const shown = useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => open,
  );
  const about = useAbout();

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    try {
      listen("show-about", openAbout).then((f) => (gone ? f() : (unlisten = f)), () => {});
    } catch {
      // Not in Tauri (the browser-only dev fixture).
    }
    return () => {
      gone = true;
      unlisten?.();
    };
  }, []);

  const reportBug = () => {
    const q = new URLSearchParams({ template: "bug_report.yml" });
    if (about) {
      q.set("version", about.commit ? `${about.version} (${about.commit})` : about.version);
      q.set("os", `${about.os} ${about.arch}`);
      if (about.git) q.set("git-version", `git version ${about.git}`);
    }
    openLink(`${REPO}/issues/new?${q}`);
  };

  const copy = () => {
    if (!about) return;
    const line = versionLine(about);
    navigator.clipboard.writeText(line).then(
      () => toast("success", "Version info copied", line),
      (e) => toast("error", "Could not copy", errorMessage(e)),
    );
  };

  return (
    <Dialog open={shown} onOpenChange={setOpen}>
      <DialogContent className="top-[16%] max-w-sm px-6 pt-8 pb-6 text-center">
        <Logo className="mx-auto size-20 drop-shadow-lg" />
        <DialogTitle className="mt-4 text-[20px] font-semibold tracking-tight">GitViber</DialogTitle>
        <DialogDescription className="mx-auto mt-1.5 max-w-64 text-[12.5px] leading-relaxed">A desktop git client for reviewing what your coding agents wrote.</DialogDescription>

        <dl className="mx-auto mt-6 grid w-fit grid-cols-[auto_auto] gap-x-3 gap-y-1.5 text-left text-[12.5px]">
          <Row label="Version">{about?.version ?? "…"}</Row>
          <Row label="Commit">
            {about?.commit ? (
              <button onClick={() => openLink(`${REPO}/commit/${about.commit}`)} className="text-primary hover:underline">
                {about.commit}
              </button>
            ) : (
              <span className="text-subtle">{about ? "unknown" : "…"}</span>
            )}
          </Row>
          <Row label="System">{about ? `${about.os} ${about.arch}` : "…"}</Row>
          <Row label="git">{about ? (about.git ?? <span className="text-destructive">not found</span>) : "…"}</Row>
        </dl>
        <button onClick={copy} disabled={!about} className="mt-2 inline-flex items-center gap-1 text-[11px] text-subtle hover:text-foreground">
          <Copy className="size-3" /> Copy for a bug report
        </button>

        <div className="mt-6 flex justify-center gap-2">
          <Button variant="secondary" size="sm" onClick={() => openLink(REPO)}>
            <ExternalLink /> GitHub
          </Button>
          <Button variant="secondary" size="sm" onClick={reportBug}>
            <Bug /> Report a Bug
          </Button>
          <Button variant="secondary" size="sm" onClick={() => openLink(`${REPO}/blob/main/LICENSE`)}>
            <Scale /> License
          </Button>
        </div>
        <div className="mt-5 text-[11px] text-subtle">© 2026 Emircan Sahin · MIT License</div>
      </DialogContent>
    </Dialog>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <dt className="text-right text-muted-foreground">{label}</dt>
      <dd className="font-mono text-foreground">{children}</dd>
    </>
  );
}
