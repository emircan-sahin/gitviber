import type { ReactNode } from "react";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { Button } from "@/components/ui/button";

/**
 * The last resort, around the whole app. A render error outside the workspace's own boundary (a
 * dialog, the shortcut overlay) unmounted everything, which left a black window that couldn't even
 * reload: ⌘R is a command, and its handler had unmounted too. The error is in the log (main.tsx).
 */
export function CrashScreen({ children }: { children: ReactNode }) {
  return (
    <ErrorBoundary
      fallback={(e) => (
        <div data-tauri-drag-region className="flex h-full flex-col items-center justify-center gap-3 bg-background p-6">
          <div className="text-[13px] font-medium">GitViber ran into an error</div>
          <pre className="max-h-[50vh] max-w-3xl overflow-auto rounded-md border border-border bg-panel p-3 font-mono text-[11px] whitespace-pre-wrap text-subtle select-text">
            {e instanceof Error ? `${e.name}: ${e.message}\n${e.stack ?? ""}` : String(e)}
          </pre>
          <Button onClick={() => location.reload()}>Reload</Button>
        </div>
      )}
    >
      {children}
    </ErrorBoundary>
  );
}
