import { Download, Loader2, RefreshCw } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { api, type GitInfo } from "@/lib/api";
import { IS_MAC } from "@/lib/platform";
import { failed, toast } from "@/lib/app/toast";

/** Takes the welcome screen's place while git can't run: nothing else works without it. */
export function NeedsGit({ info, onRecheck }: { info: GitInfo; onRecheck: () => Promise<void> }) {
  const [checking, setChecking] = useState(false);
  const mac = info.state === "tools" || IS_MAC;
  const recheck = async () => {
    setChecking(true);
    try {
      await onRecheck();
    } finally {
      setChecking(false);
    }
  };
  const install = () =>
    api.installGit().then(
      () => toast("info", "Follow the installer macOS opened", "Then come back and click Check again."),
      failed("Could not start the installer"),
    );
  return (
    <div data-tauri-drag-region className="flex h-full items-center justify-center bg-background">
      <div className="w-[400px]">
        <div className="flex items-center gap-3">
          <img src="/icon.svg" alt="" className="size-11" />
          <div>
            <h1 className="text-[20px] font-semibold tracking-tight">GitViber needs git</h1>
            <p className="text-[12px] text-muted-foreground">
              {info.state === "tools" ? "git comes with Apple's Command Line Tools, which aren't installed yet." : "git isn't installed, or GitViber can't find it."}
            </p>
          </div>
        </div>
        {mac && (
          <Button size="lg" className="mt-6 w-full justify-start" onClick={install}>
            <Download /> Install Command Line Tools
          </Button>
        )}
        <p className="mt-3 text-[12px] text-muted-foreground">
          {mac ? (
            <>
              Or with Homebrew: <code className="rounded-sm bg-panel px-1 py-0.5 font-mono text-[11.5px] text-foreground select-text">brew install git</code>
            </>
          ) : (
            "Install git with your package manager or from git-scm.com."
          )}
        </p>
        <Button variant="secondary" size="lg" className="mt-4 w-full justify-start" disabled={checking} onClick={recheck}>
          {checking ? <Loader2 className="animate-spin" /> : <RefreshCw />} Check again
        </Button>
        {info.detail && <pre className="mt-4 max-h-32 overflow-auto rounded-md border border-border bg-panel p-2.5 font-mono text-[11px] whitespace-pre-wrap text-subtle select-text">{info.detail}</pre>}
      </div>
    </div>
  );
}
