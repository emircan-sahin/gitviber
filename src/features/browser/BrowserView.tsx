import { useEffect, useRef, useState } from "react";
import { browserApi, errorMessage } from "@/lib/api";
import { pageLabel } from "@/lib/browser/url";
import { commandIn } from "@/lib/commands/keybindings";
import type { Selection } from "@/lib/repo/selection";
import { Placeholder } from "@/features/viewer/FileHeader";
import type { Tab } from "@/features/viewer/tabs";
import { AddressBar } from "./AddressBar";
import { onBrowserKey, setBrowserState, useBrowserState } from "./store";
import { useNativeRect } from "./useNativeRect";

type BrowserSelection = Extract<Selection, { kind: "browser" }>;

/** The tab's own keys, in its address bar or its page. */
const OWN = ["browser.focusAddress", "browser.reload", "browser.back", "browser.forward"] as const;

/**
 * A web page in a tab: the address bar here, the page a native view of its own laid over the area
 * below (browser/macos.rs). It's made as the tab first shows and lives on hidden while another
 * does; closing the tab closes it (useTabs).
 */
export function BrowserView({ tab, sel, root, onUpdate }: { tab: Tab; sel: BrowserSelection; root: string; onUpdate: (key: string, sel: Selection) => void }) {
  const { id } = sel;
  const state = useBrowserState(id);
  const [made, setMade] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const area = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);

  // Only on the first show and on its own id: where it loads next is the page's business.
  useEffect(() => {
    let live = true;
    browserApi.create(id, root, sel.url).then(
      (s) => {
        if (!live) return;
        setBrowserState(s);
        setMade(true);
        // A new tab starts at its address bar.
        if (s.url === "" || s.url === "about:blank") field.current?.focus();
      },
      (e) => live && setError(errorMessage(e)),
    );
    return () => {
      live = false;
      void browserApi.hide(id).catch(() => {});
    };
  }, [id]);

  const cover = useNativeRect(area, id, made && !state?.failed);

  // The tab keeps where the page went, for the next launch: once it settles, not every redirect.
  const url = state?.url;
  const title = state?.title;
  useEffect(() => {
    if (!url || (url === sel.url && (title || undefined) === sel.title)) return;
    const timer = setTimeout(() => onUpdate(tab.key, { ...sel, url, title: title || undefined }), 1000);
    return () => clearTimeout(timer);
  }, [url, title, sel, tab.key, onUpdate]);

  const run = (command: (typeof OWN)[number]) => {
    if (command === "browser.focusAddress") {
      // Keys come back to this page first, or they'd still go to the page's view.
      void browserApi
        .focus(id, false)
        .catch(() => {})
        .then(() => {
          field.current?.focus();
          field.current?.select();
        });
    } else {
      void browserApi.go(id, command === "browser.reload" ? "reload" : command === "browser.back" ? "back" : "forward").catch(() => {});
    }
  };
  const runOwn = (e: KeyboardEvent) => {
    const command = commandIn(OWN, e);
    if (command) run(command);
    return !!command;
  };
  useEffect(() => onBrowserKey(id, runOwn));

  const failed = state?.failed;
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      onKeyDown={(e) => {
        if (!runOwn(e.nativeEvent)) return;
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      <AddressBar id={id} url={sel.url} state={state} field={field} />
      <div ref={area} className="relative min-h-0 flex-1 bg-background">
        {error ? (
          <Placeholder title="The browser can't open here" detail={error} />
        ) : failed ? (
          <Placeholder title={`Can't open ${pageLabel(failed.url)}`} detail={failed.message} action={{ label: "Try Again", run: () => void browserApi.navigate(id, failed.url).catch(() => {}) }} />
        ) : (
          cover && <img src={cover} alt="" className="absolute inset-0 size-full object-cover select-none" />
        )}
      </div>
    </div>
  );
}
