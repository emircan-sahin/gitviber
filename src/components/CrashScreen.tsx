import { Component, type ReactNode } from "react";
import { Button } from "@/components/ui/button";

/**
 * The last resort, around the whole app. A render error outside the workspace's own boundary (a
 * dialog, the shortcut overlay) unmounted everything, which left a black window that couldn't even
 * reload: ⌘R is a command, and its handler had unmounted too. The error is in the log (main.tsx).
 */
export class CrashScreen extends Component<{ children: ReactNode }, { error: string | null }> {
  state = { error: null as string | null };
  static getDerivedStateFromError(e: unknown) {
    return { error: e instanceof Error ? `${e.name}: ${e.message}\n${e.stack ?? ""}` : String(e) };
  }
  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div data-tauri-drag-region className="flex h-full flex-col items-center justify-center gap-3 bg-background p-6">
        <div className="text-[13px] font-medium">GitViber ran into an error</div>
        <pre className="max-h-[50vh] max-w-3xl overflow-auto rounded-md border border-border bg-panel p-3 font-mono text-[11px] whitespace-pre-wrap text-subtle select-text">{this.state.error}</pre>
        <Button onClick={() => location.reload()}>Reload</Button>
      </div>
    );
  }
}
