import { Bug, ChevronRight, Copy, ExternalLink, Scale } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Logo } from "@/components/Logo";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { type About, api, errorMessage, github } from "@/lib/api";
import { useCommands } from "@/lib/commands/keybindings";
import { failed, toast } from "@/lib/app/toast";
import { cn } from "@/lib/utils";
import { copyText } from "@/lib/app/clipboard";
import { createStore } from "@/lib/store";

export const REPO = "https://github.com/emircan-sahin/gitviber";

let aboutOnce: Promise<About> | null = null;

/** Version, commit, OS and git; asked once per session, and again on `ask` (git may have been upgraded). */
export function useAbout(ask = false) {
  const [about, setAbout] = useState<About | null>(null);
  useEffect(() => {
    if (ask) aboutOnce = api.about();
    aboutOnce ??= api.about();
    aboutOnce.then(setAbout, () => {});
  }, [ask]);
  return about;
}

/** What a bug report needs to know about the setup, in one line. */
export function diagnostics(a: About) {
  const build = a.commit ? ` (${a.commit})` : "";
  return [`GitViber ${a.version}${build}`, `${a.os} ${a.arch}`, `git ${a.git ?? "not found"}`, `gh ${a.gh ?? "not found"}`, a.webview].filter(Boolean).join(" · ");
}

const open = createStore(false);
const setOpen = open.set;
export const openAbout = () => setOpen(true);

const openLink = (url: string) => github.openUrl(url).catch(failed("Could not open the link"));

/** GitViber → About GitViber in the menu bar, and the version in the status bar. */
export function AboutDialog() {
  const shown = open.use();
  const about = useAbout(shown);
  const [licenses, setLicenses] = useState(false);

  const reportBug = () => {
    const q = new URLSearchParams({ template: "bug_report.yml" });
    if (about) {
      q.set("version", about.commit ? `${about.version} (${about.commit})` : about.version);
      q.set("os", `${about.os} ${about.arch}`);
      if (about.git) q.set("git-version", `git version ${about.git}`);
    }
    openLink(`${REPO}/issues/new?${q}`);
  };

  useCommands({
    "app.about": openAbout,
    "help.readme": () => openLink(`${REPO}#readme`),
    "help.reportBug": reportBug,
    "help.copyDiagnostics": () => copy(),
    "help.showLogs": () => api.showLogs().catch(failed("Could not show the logs")),
    "app.installCli": () =>
      api.installCli().then((path) => toast("success", "Installed the gitviber command", `${path}: run gitviber in a repository to open it here.`), failed("Could not install the command")),
    "help.releaseNotes": () => openLink(`${REPO}/releases`),
    "help.license": () => openLink(`${REPO}/blob/main/LICENSE`),
  });

  // Synchronous: WebKit lets the page write the clipboard only while handling a click or key.
  const copy = () => {
    if (!about) return toast("error", "Could not copy", "The version info hasn't loaded.");
    const line = diagnostics(about);
    void copyText(line, "Diagnostics copied", line);
  };

  return (
    <Dialog
      open={shown}
      onOpenChange={(o) => {
        setOpen(o);
        if (!o) setLicenses(false);
      }}
    >
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
          <Row label="gh">{about ? (about.gh ?? <span className="text-subtle">not installed</span>) : "…"}</Row>
          <Row label="Web view">{about ? (about.webview ?? <span className="text-subtle">unknown</span>) : "…"}</Row>
        </dl>
        <button onClick={copy} disabled={!about} className="mt-2 inline-flex items-center gap-1 text-[11px] text-subtle hover:text-foreground focus-visible:text-foreground">
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
        <div className="mt-5 text-[11px] text-subtle">
          © 2026 Emircan Sahin · GPL-3.0 License ·{" "}
          <button onClick={() => setLicenses(true)} className="hover:text-foreground focus-visible:text-foreground hover:underline">
            Third-Party Licenses
          </button>
        </div>
        <LicensesDialog open={licenses} onOpenChange={setLicenses} />
      </DialogContent>
    </Dialog>
  );
}

/** Written by scripts/licenses.mjs (`pnpm licenses:generate`). */
type Licenses = {
  packages: { name: string; version: string; license: string; url: string; source: "npm" | "cargo"; standard?: string; texts: number[] }[];
  texts: string[];
};

let licensesOnce: Promise<Licenses> | null = null;

/** Every npm and cargo package the app ships, with its license text; ~900 KB, so loaded on open. */
function LicensesDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const [data, setData] = useState<Licenses | null>(null);
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    licensesOnce ??= import("@/lib/third-party-licenses.json").then((m) => m.default as Licenses);
    licensesOnce.then(setData, (e) => {
      licensesOnce = null;
      toast("error", "Could not load the licenses", errorMessage(e));
    });
  }, [open]);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (data?.packages ?? []).filter((p) => !q || p.name.toLowerCase().includes(q) || p.license.toLowerCase().includes(q));
  }, [data, query]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="top-[10%] flex h-[80vh] max-w-2xl flex-col text-left">
        <DialogTitle>Third-Party Licenses</DialogTitle>
        <DialogDescription>GitViber is built on these open-source packages. Click one to read its license.</DialogDescription>
        <Input autoFocus value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Filter by name or license" className="mt-3 shrink-0" />
        <div className="mt-2 min-h-0 flex-1 overflow-y-auto">
          {!data ? (
            <div className="py-6 text-center text-[12px] text-subtle">Loading…</div>
          ) : shown.length === 0 ? (
            <div className="py-6 text-center text-[12px] text-subtle">No packages match</div>
          ) : (
            shown.map((p) => {
              const key = `${p.source}:${p.name}@${p.version}`;
              const isOpen = expanded === key;
              return (
                <div key={key}>
                  <button
                    onClick={() => setExpanded(isOpen ? null : key)}
                    className={cn("flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-[12px] hover:bg-hover focus-visible:bg-hover", isOpen && "bg-hover")}
                  >
                    <ChevronRight className={cn("size-3 shrink-0 text-subtle transition-transform", isOpen && "rotate-90")} />
                    <span className="min-w-0 truncate">{p.name}</span>
                    <span className="shrink-0 font-mono text-[11px] text-subtle">{p.version}</span>
                    <span className="ml-auto shrink-0 truncate pl-3 text-[11px] text-muted-foreground">{p.license}</span>
                    <span className="w-9 shrink-0 text-right text-[10.5px] text-subtle">{p.source === "npm" ? "npm" : "crate"}</span>
                  </button>
                  {isOpen && (
                    <div className="mb-2 ml-7 mr-2 mt-1 space-y-2">
                      <button onClick={() => openLink(p.url)} className="inline-flex items-center gap-1 text-[11px] text-primary hover:underline">
                        <ExternalLink className="size-3" /> {p.url}
                      </button>
                      {p.standard && <p className="text-[11px] text-subtle">This package ships no license file, so this is the standard {p.standard} text.</p>}
                      {p.texts.map((t) => (
                        <pre key={t} className="select-text whitespace-pre-wrap break-words rounded-sm border border-border bg-background p-2.5 font-mono text-[10.5px] leading-relaxed text-muted-foreground">
                          {data.texts[t]}
                        </pre>
                      ))}
                    </div>
                  )}
                </div>
              );
            })
          )}
        </div>
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
