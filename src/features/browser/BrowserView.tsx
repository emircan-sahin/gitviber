import { useEffect, useRef, useState } from "react";
import { browserApi, type BrowserGo, errorMessage, github } from "@/lib/api";
import { failed, toast } from "@/lib/app/toast";
import { onBrowserKey, setBrowserState, useBrowserState, useParks } from "@/lib/browser/store";
import { BLANK, isFrameable, pageLabel } from "@/lib/browser/url";
import { commandIn, useCommands } from "@/lib/commands/keybindings";
import { IS_LINUX } from "@/lib/platform";
import type { Selection } from "@/lib/repo/selection";
import { Placeholder } from "@/features/viewer/FileHeader";
import { AddressBar, type PageControl } from "./AddressBar";
import { useNativeRect } from "./useNativeRect";

type BrowserSelection = Extract<Selection, { kind: "browser" }>;

interface Props {
  tabKey: string;
  sel: BrowserSelection;
  root: string;
  onUpdate: (key: string, sel: Selection) => void;
}

/** The tab's own keys, in its address bar or its page; the GO ones go to the page. */
const GO = { "browser.reload": "reload", "browser.back": "back", "browser.forward": "forward" } as const satisfies Record<string, BrowserGo>;
const OWN = ["browser.focusAddress", "browser.inspect", ...(Object.keys(GO) as (keyof typeof GO)[])] as const;

/** A web page in a tab: a browser view of its own on macOS, a frame of this page's on Linux. */
export function BrowserView(props: Props) {
  return IS_LINUX ? <FramePage {...props} /> : <NativePage {...props} />;
}

/**
 * The address bar here, the page a native view laid over the area below (browser/macos.rs). It's
 * made as the tab first shows and lives on hidden while another does, until it parks (Settings →
 * Browser); closing the tab closes it (useTabs).
 */
function NativePage({ tabKey, sel, root, onUpdate }: Props) {
  const { id } = sel;
  const state = useBrowserState(id);
  const parks = useParks(id);
  const [made, setMade] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A parked page's picture, standing in until it has loaded again.
  const [restoring, setRestoring] = useState<string | null>(null);
  const area = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);

  // On the first show and after each park, on its own id: where it loads next is the page's business.
  useEffect(() => {
    let live = true;
    setMade(false);
    browserApi.create(id, root, sel.url).then(
      ({ snapshot, ...s }) => {
        if (!live) return;
        setBrowserState(s);
        setRestoring(snapshot);
        setMade(true);
        // A new tab starts at its address bar.
        if (sel.url === BLANK) field.current?.focus();
      },
      (e) => live && setError(errorMessage(e)),
    );
    return () => {
      live = false;
      void browserApi.hide(id, false).catch(() => {});
    };
  }, [id, parks]);

  // Its first load has committed (or failed): the page paints from here.
  const loaded = !!state && (state.committed || !!state.failed);
  useEffect(() => {
    if (restoring && loaded) setRestoring(null);
  }, [restoring, loaded]);

  const cover = useNativeRect(area, id, made && !state?.failed && !restoring);

  // The tab keeps where the page went, for the next launch: once it settles, not every redirect.
  const url = state?.url;
  const title = state?.title;
  useEffect(() => {
    if (!url || (url === sel.url && (title || undefined) === sel.title)) return;
    const timer = setTimeout(() => onUpdate(tabKey, { ...sel, url, title: title || undefined }), 1000);
    return () => clearTimeout(timer);
  }, [url, title, sel, tabKey, onUpdate]);

  const inspect = () =>
    void browserApi
      .inspect(id)
      .then((shown) => shown || toast("info", "Web Inspector", "Right-click the page and choose Inspect Element."))
      .catch(failed("Could not open Web Inspector"));
  useCommands({ "browser.inspect": made ? inspect : undefined });

  /** One of the tab's own keys, run; false for any other key. */
  const runOwn = (e: KeyboardEvent) => {
    const command = commandIn(OWN, e);
    if (!command) return false;
    if (command === "browser.inspect") inspect();
    else if (command !== "browser.focusAddress") void browserApi.go(id, GO[command]).catch(() => {});
    else {
      // Keys come back to this page first, or they'd still go to the page's view.
      void browserApi
        .focus(id, false)
        .catch(() => {})
        .then(() => {
          field.current?.focus();
          field.current?.select();
        });
    }
    return true;
  };
  // runOwn reads only the id and refs.
  useEffect(() => onBrowserKey(id, runOwn), [id]);

  const page: PageControl = {
    navigate: (next) => void browserApi.navigate(id, next).catch(failed("Could not open the page")),
    go: (to) => void browserApi.go(id, to).catch(failed("The page didn't respond")),
    focus: () => void browserApi.focus(id, true).catch(() => {}),
  };
  const notLoaded = state?.failed;
  const picture = restoring ?? cover;
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      onKeyDown={(e) => {
        // Typed while the page steps aside (useNativeRect): the page's, so no app command runs.
        const forPage = e.target === area.current && !e.metaKey && !e.ctrlKey;
        if (!runOwn(e.nativeEvent) && !forPage) return;
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      <AddressBar url={sel.url} state={state} page={page} root={root} field={field} />
      <div ref={area} tabIndex={-1} className="relative min-h-0 flex-1 bg-background outline-none">
        {error ? (
          <Placeholder title="The browser can't open here" detail={error} />
        ) : notLoaded ? (
          <Placeholder title={`Can't open ${pageLabel(notLoaded.url)}`} detail={notLoaded.message} action={{ label: "Try Again", run: () => page.navigate(notLoaded.url) }} />
        ) : (
          picture && <img src={picture} alt="" className="absolute inset-0 size-full object-cover select-none" />
        )}
      </div>
    </div>
  );
}

/**
 * Linux: the page in a frame of this page's, for http(s) on localhost and 127.0.0.1 only (the
 * CSP's frame-src). A frame's page can't be followed from here: no back or forward, and its links
 * don't show in the address bar. Sandboxed: it can't navigate this page away.
 */
function FramePage({ tabKey, sel, root, onUpdate }: Props) {
  const [url, setUrl] = useState(sel.url);
  const [loads, setLoads] = useState(0);
  const frame = useRef<HTMLIFrameElement>(null);
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (sel.url === BLANK) field.current?.focus();
  }, []);
  useEffect(() => {
    if (url !== sel.url) onUpdate(tabKey, { ...sel, url, title: undefined });
  }, [url, sel, tabKey, onUpdate]);
  const page: PageControl = {
    navigate: (next) => {
      setUrl(next);
      setLoads((n) => n + 1);
    },
    go: (to) => (to === "reload" || to === "hardReload") && setLoads((n) => n + 1),
    focus: () => frame.current?.focus(),
  };
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <AddressBar url={url} state={null} page={page} root={root} field={field} />
      <div className="relative min-h-0 flex-1 bg-background">
        {url === BLANK ? null : isFrameable(url) ? (
          <iframe
            key={loads}
            ref={frame}
            src={url}
            title={pageLabel(url)}
            sandbox="allow-scripts allow-same-origin allow-forms allow-modals allow-popups allow-downloads"
            className="size-full border-0 bg-white"
          />
        ) : (
          <Placeholder
            title="On Linux, a browser tab opens localhost and 127.0.0.1 only"
            detail={url}
            action={{ label: "Open in Browser", run: () => void github.openUrl(url).catch(failed("Could not open the link")) }}
          />
        )}
      </div>
    </div>
  );
}
