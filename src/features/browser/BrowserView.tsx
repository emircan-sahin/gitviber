import { TabletSmartphone } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Tip } from "@/components/ui/tooltip";
import { browserApi, type BrowserGo, errorMessage, github } from "@/lib/api";
import { failed, toast } from "@/lib/app/toast";
import { onBrowserKey, setBrowserState, useBrowserState, useParks } from "@/lib/browser/store";
import { RESPONSIVE } from "@/lib/browser/devices";
import type { Fitted } from "@/lib/browser/fit";
import { BLANK, isFrameable, pageLabel } from "@/lib/browser/url";
import { commandIn, useCommands, useShortcut } from "@/lib/commands/keybindings";
import { cn } from "@/lib/utils";
import { IS_LINUX } from "@/lib/platform";
import type { Selection } from "@/lib/repo/selection";
import { Placeholder } from "@/features/viewer/FileHeader";
import { AddressBar, type PageControl } from "./AddressBar";
import { DeviceBar } from "./DeviceBar";
import { DeviceFrame } from "./DeviceFrame";
import { useDevice } from "./useDevice";
import { type PageScreen, useNativeRect } from "./useNativeRect";

type BrowserSelection = Extract<Selection, { kind: "browser" }>;

interface Props {
  tabKey: string;
  sel: BrowserSelection;
  root: string;
  onUpdate: (key: string, sel: Selection) => void;
}

/** The tab's own keys, in its address bar or its page; the GO ones go to the page. */
const GO = { "browser.reload": "reload", "browser.back": "back", "browser.forward": "forward" } as const satisfies Record<string, BrowserGo>;
const OWN = ["browser.focusAddress", "browser.inspect", "browser.toggleDevice", ...(Object.keys(GO) as (keyof typeof GO)[])] as const;

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
  const screenEl = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const { choice, device, choose, toggle } = useDevice(tabKey, sel, onUpdate);
  const [fitted, setFitted] = useState<Fitted | null>(null);
  // Whether WebKit lets the page see the device's pixel ratio here.
  const [dpr, setDpr] = useState(true);
  const ua = device?.ua || null;

  // On the first show and after each park, on its own id: where it loads next is the page's business.
  useEffect(() => {
    let live = true;
    setMade(false);
    browserApi.create(id, root, sel.url, ua).then(
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

  // A device's user agent: the page loads again under it (rust: set_agent).
  useEffect(() => {
    if (made) void browserApi.setAgent(id, ua).then(setDpr, () => {});
  }, [made, id, ua]);

  const screen: PageScreen | null = device && fitted && { scale: fitted.scale, radius: fitted.radius, dpr: device.dpr || null, cutout: fitted.cutout };
  const cover = useNativeRect(device ? screenEl : area, id, made && !state?.failed && !restoring && (!device || !!fitted), screen);

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
    else if (command === "browser.toggleDevice") toggleRef.current();
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
  const toggleRef = useRef(toggle);
  toggleRef.current = toggle;
  useCommands({ "browser.toggleDevice": made ? toggle : undefined });

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
        const forPage = (e.target === area.current || e.target === screenEl.current) && !e.metaKey && !e.ctrlKey;
        if (!runOwn(e.nativeEvent) && !forPage) return;
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      <AddressBar url={sel.url} state={state} page={page} root={root} field={field} tools={<DeviceToggle on={!!choice} onToggle={toggle} />} />
      {choice && device && <DeviceBar choice={choice} device={device} dpr={dpr} onChoice={choose} />}
      <div ref={area} tabIndex={-1} className="relative min-h-0 flex-1 bg-background outline-none">
        {error ? (
          <Placeholder title="The browser can't open here" detail={error} />
        ) : notLoaded ? (
          <Placeholder title={`Can't open ${pageLabel(notLoaded.url)}`} detail={notLoaded.message} action={{ label: "Try Again", run: () => page.navigate(notLoaded.url) }} />
        ) : device && choice ? (
          <DeviceFrame device={device} rotated={!!choice.rotated} screen={screenEl} onFit={setFitted} onResize={choice.name === RESPONSIVE ? (size) => choose({ ...choice, ...size }) : undefined}>
            {picture && <img src={picture} alt="" className="absolute inset-0 size-full object-cover" />}
          </DeviceFrame>
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
  const screenEl = useRef<HTMLDivElement>(null);
  const { choice, device, choose, toggle } = useDevice(tabKey, sel, onUpdate);
  const [fitted, setFitted] = useState<Fitted | null>(null);
  useEffect(() => {
    if (sel.url === BLANK) field.current?.focus();
  }, []);
  useEffect(() => {
    if (url !== sel.url) onUpdate(tabKey, { ...sel, url, title: undefined });
  }, [url, sel, tabKey, onUpdate]);
  useCommands({ "browser.toggleDevice": toggle });
  const page: PageControl = {
    navigate: (next) => {
      setUrl(next);
      setLoads((n) => n + 1);
    },
    go: (to) => (to === "reload" || to === "hardReload") && setLoads((n) => n + 1),
    focus: () => frame.current?.focus(),
  };
  // In device mode the frame is the device's size and scaled to its screen: no user agent or
  // pixel ratio of its own here.
  const sized = device && fitted && { width: fitted.viewport.w, height: fitted.viewport.h, transform: `scale(${fitted.scale})`, transformOrigin: "0 0" };
  const content =
    url === BLANK ? null : isFrameable(url) ? (
      <iframe
        key={loads}
        ref={frame}
        src={url}
        title={pageLabel(url)}
        sandbox="allow-scripts allow-same-origin allow-forms allow-modals allow-popups allow-downloads"
        className={cn("border-0 bg-white", !sized && "size-full")}
        style={sized || undefined}
      />
    ) : (
      <Placeholder
        title="On Linux, a browser tab opens localhost and 127.0.0.1 only"
        detail={url}
        action={{ label: "Open in Browser", run: () => void github.openUrl(url).catch(failed("Could not open the link")) }}
      />
    );
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <AddressBar url={url} state={null} page={page} root={root} field={field} tools={<DeviceToggle on={!!choice} onToggle={toggle} />} />
      {choice && device && <DeviceBar choice={choice} device={device} dpr={false} onChoice={choose} />}
      <div className="relative min-h-0 flex-1 bg-background">
        {device && choice && isFrameable(url) ? (
          <DeviceFrame device={device} rotated={!!choice.rotated} screen={screenEl} onFit={setFitted} onResize={choice.name === RESPONSIVE ? (size) => choose({ ...choice, ...size }) : undefined}>
            {content}
          </DeviceFrame>
        ) : (
          content
        )}
      </div>
    </div>
  );
}

/** Device mode on or off, in the address bar. */
function DeviceToggle({ on, onToggle }: { on: boolean; onToggle: () => void }) {
  return (
    <Tip label={on ? "Leave device mode" : "Device mode"} shortcut={useShortcut("browser.toggleDevice")}>
      <button
        type="button"
        aria-pressed={on}
        aria-label="Device mode"
        onClick={onToggle}
        className={cn(
          "flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground outline-none hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground",
          on && "bg-active text-foreground",
        )}
      >
        <TabletSmartphone className="size-3.5" />
      </button>
    </Tip>
  );
}
