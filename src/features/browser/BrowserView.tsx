import { Crosshair, type LucideIcon, Moon, Sun, SunMoon, TabletSmartphone } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { browserApi, type BrowserGo, type BrowserPick, errorMessage, github } from "@/lib/api";
import { failed, toast } from "@/lib/app/toast";
import { formatErrors, type ShownAs } from "@/lib/browser/format";
import { onBrowserKey, setBrowserState, useBrowserState, useLogged, useParks } from "@/lib/browser/store";
import { RESPONSIVE } from "@/lib/browser/devices";
import { type Scheme, zoomLabel } from "@/lib/browser/look";
import { BLANK, isFrameable, pageLabel } from "@/lib/browser/url";
import { type CommandId, commandIn, useCommands, useShortcut } from "@/lib/commands/keybindings";
import { IS_LINUX } from "@/lib/platform";
import { useSettings } from "@/lib/settings";
import type { Selection } from "@/lib/repo/selection";
import { Placeholder } from "@/features/viewer/FileHeader";
import { AddressBar, type PageControl } from "./AddressBar";
import { ConsoleBadge, ConsolePanel } from "./ConsolePanel";
import { AskPopover, picksAbout } from "./AskPopover";
import { DeviceBar } from "./DeviceBar";
import { DeviceFrame } from "./DeviceFrame";
import { useDevice } from "./useDevice";
import { useDeviceFit } from "./useDeviceFit";
import { useNativeRect } from "./useNativeRect";
import { usePicker } from "./usePicker";
import { FindBar, useFindInPage } from "./FindBar";
import { usePageLook } from "./usePageLook";
import { useFind } from "@/lib/ui/find";

type BrowserSelection = Extract<Selection, { kind: "browser" }>;

interface Props {
  tabKey: string;
  sel: BrowserSelection;
  root: string;
  onUpdate: (key: string, sel: Selection) => void;
}

/** The tab's own keys, in its address bar or its page; the GO ones go to the page. */
const GO = { "browser.reload": "reload", "browser.back": "back", "browser.forward": "forward" } as const satisfies Record<string, BrowserGo>;
const OWN = [
  "browser.focusAddress",
  "browser.inspect",
  "browser.toggleDevice",
  "browser.pick",
  "browser.find",
  "browser.findNext",
  "browser.findPrev",
  "browser.zoomIn",
  "browser.zoomOut",
  "browser.zoomReset",
  ...(Object.keys(GO) as (keyof typeof GO)[]),
] as const;
type Own = Exclude<(typeof OWN)[number], keyof typeof GO>;

const TOO_SMALL = "Too little room to show the device";

/** A web page in a tab: a browser view of its own on macOS, a frame of this page's on Linux. */
export function BrowserView(props: Props) {
  return IS_LINUX ? <FramePage {...props} /> : <NativePage {...props} />;
}

/** Device mode's choice and layout for a tab, Responsive's size kept as a drag ends. */
function useTabDevice({ tabKey, sel, onUpdate }: Props, area: React.RefObject<HTMLDivElement | null>) {
  const { choice, choose, toggle } = useDevice(tabKey, sel, onUpdate);
  const { device, fitted, resize } = useDeviceFit(area, choice, (size) => choice && choose({ ...choice, ...size }));
  return { choice, choose, toggle, device, fitted, resize: choice?.name === RESPONSIVE ? resize : undefined };
}

/**
 * The address bar here, the page a native view laid over the area below (browser/macos/). It's
 * made as the tab first shows and lives on hidden while another does, until it parks (Settings →
 * Browser); closing the tab closes it (useTabs).
 */
function NativePage(props: Props) {
  const { tabKey, sel, root, onUpdate } = props;
  const { id } = sel;
  const state = useBrowserState(id);
  const parks = useParks(id);
  const [made, setMade] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A parked page's picture, standing in until it has loaded again.
  const [restoring, setRestoring] = useState<string | null>(null);
  // Whether WebKit lets the page see a device's pixel ratio here.
  const [dpr, setDpr] = useState(true);
  const area = useRef<HTMLDivElement>(null);
  const pageEl = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const { choice, choose, toggle, device, fitted, resize } = useTabDevice(props, area);
  const ua = device?.ua || null;
  const { browserConsole: capture, uiScale } = useSettings();
  const [consoleOpen, setConsoleOpen] = useState(false);
  const { picking, picks, toggle: togglePick, closeAsk } = usePicker(id);
  const errorCount = useLogged(id).errors;
  const find = useFindInPage(id);
  const look = usePageLook(tabKey, sel, onUpdate, made);

  // On the first show and after each park, on its own id: where it loads next is the page's business.
  useEffect(() => {
    let live = true;
    setMade(false);
    // As the tab shows it from the first paint; usePageLook keeps it so after.
    browserApi.create(id, root, sel.url, ua, sel.zoom ?? null, sel.scheme ? sel.scheme === "dark" : null).then(
      ({ snapshot, dpr: canSetDpr, ...s }) => {
        if (!live) return;
        setBrowserState(s);
        setRestoring(snapshot);
        setDpr(canSetDpr);
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

  // A device's user agent: the page loads again under it (browser/macos set_agent).
  useEffect(() => {
    if (made) void browserApi.setAgent(id, ua).catch(failed("Could not show the page as the device"));
  }, [made, id, ua]);

  const shown = !device || !!fitted?.fits;
  const screen = device && fitted && { viewport: fitted.viewport, radius: fitted.radius, corners: fitted.corners, dpr: device.dpr || null };
  const cover = useNativeRect(device ? pageEl : area, id, made && !state?.failed && !restoring && shown, screen);

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
      .then((opened) => opened || toast("info", "Web Inspector", "Right-click the page and choose Inspect Element."))
      .catch(failed("Could not open Web Inspector"));
  // Device mode zooms the page to fit, so its own zoom waits until that's off.
  const zoomBy = (by: -1 | 0 | 1) => !choice && look.zoomBy(by);
  const own: Record<Own, () => void> = {
    "browser.focusAddress": () =>
      // Keys come back to this page first, or they'd still go to the page's view.
      void browserApi
        .focus(id, false)
        .catch(() => {})
        .then(() => {
          field.current?.focus();
          field.current?.select();
        }),
    "browser.inspect": inspect,
    "browser.toggleDevice": toggle,
    "browser.pick": togglePick,
    "browser.find": find.show,
    "browser.findNext": () => find.step(false),
    "browser.findPrev": () => find.step(true),
    "browser.zoomIn": () => zoomBy(1),
    "browser.zoomOut": () => zoomBy(-1),
    "browser.zoomReset": () => zoomBy(0),
  };
  useCommands(made ? own : {});
  // Find from the menu, or with focus elsewhere in the code view, finds in the page.
  useFind("code", made ? find.show : null);

  // The page's keys arrive through onBrowserKey, registered once: the latest actions are read here.
  const ownRef = useRef(own);
  ownRef.current = own;
  /** One of the tab's own keys, run; false for any other key. */
  const runOwn = (e: KeyboardEvent) => {
    const command = commandIn(OWN, e);
    if (!command) return false;
    if (command in GO) void browserApi.go(id, GO[command as keyof typeof GO]).catch(() => {});
    else ownRef.current[command as Own]();
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
  const shownAs: ShownAs | null = device && fitted ? { name: device.name, viewport: fitted.viewport } : null;
  // The picked elements on the page's picture while their question is asked, in this page's px:
  // a page px is the device's scale here in device mode, else the page's zoom over the interface's.
  const factor = device && fitted ? fitted.scale : look.zoom / uiScale;
  const mark = (p: BrowserPick, i: number) => (
    <div
      key={i}
      className="pointer-events-none absolute rounded-[2px] border-2 border-primary bg-primary/15"
      style={{ left: p.box.x * factor, top: p.box.y * factor, width: Math.max(p.box.w * factor, 2), height: Math.max(p.box.h * factor, 2) }}
    />
  );
  const asked = picks && (
    <>
      {picks.slice(0, -1).map(mark)}
      <AskPopover
        root={root}
        about={picksAbout(picks, shownAs)}
        pageErrors={{ count: errorCount, read: () => browserApi.console(id).then((entries) => formatErrors(entries, { url: state?.url ?? sel.url, device: shownAs })) }}
        onClose={closeAsk}
      >
        {mark(picks[picks.length - 1], picks.length - 1)}
      </AskPopover>
    </>
  );
  const image = (
    <>
      {picture && <img src={picture} alt="" className="absolute inset-0 size-full object-cover select-none" />}
      {asked}
    </>
  );
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      onKeyDown={(e) => {
        // Typed while the page steps aside (useNativeRect): the page's, so no app command runs.
        const forPage = (e.target === area.current || e.target === pageEl.current) && !e.metaKey && !e.ctrlKey;
        if (!runOwn(e.nativeEvent) && !forPage) return;
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      <AddressBar
        url={sel.url}
        state={state}
        page={page}
        root={root}
        field={field}
        tools={
          <>
            <ToolToggle tool={PICK} on={picking} onToggle={togglePick} />
            {capture && <ConsoleBadge id={id} open={consoleOpen} onToggle={() => setConsoleOpen((o) => !o)} />}
            {!choice && look.zoom !== 1 && <ZoomChip zoom={look.zoom} onReset={() => look.zoomBy(0)} />}
            <SchemeToggle scheme={look.scheme} onCycle={look.cycleScheme} />
            <ToolToggle tool={DEVICE} on={!!choice} onToggle={toggle} />
          </>
        }
      />
      {choice && device && <DeviceBar choice={choice} device={device} viewport={fitted?.viewport ?? null} dpr={dpr} onChoice={choose} />}
      {find.open && <FindBar find={find} />}
      {capture && consoleOpen && (
        <ConsolePanel id={id} root={root} page={{ url: state?.url ?? sel.url, device: shownAs }} onAsk={closeAsk} onClose={() => setConsoleOpen(false)} />
      )}
      <div ref={area} tabIndex={-1} className="relative min-h-0 flex-1 bg-background outline-none">
        {error ? (
          <Placeholder title="The browser can't open here" detail={error} />
        ) : notLoaded ? (
          <Placeholder title={`Can't open ${pageLabel(notLoaded.url)}`} detail={notLoaded.message} action={{ label: "Try Again", run: () => page.navigate(notLoaded.url) }} />
        ) : device && choice ? (
          fitted &&
          (fitted.fits ? (
            <DeviceFrame device={device} rotated={!!choice.rotated} fitted={fitted} page={pageEl} resize={resize}>
              {image}
            </DeviceFrame>
          ) : (
            <Placeholder title={TOO_SMALL} />
          ))
        ) : (
          image
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
function FramePage(props: Props) {
  const { tabKey, sel, root, onUpdate } = props;
  const [url, setUrl] = useState(sel.url);
  const [loads, setLoads] = useState(0);
  const frame = useRef<HTMLIFrameElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const area = useRef<HTMLDivElement>(null);
  const pageEl = useRef<HTMLDivElement>(null);
  const { choice, choose, toggle, device, fitted, resize } = useTabDevice(props, area);
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
  // In device mode the frame is the page's viewport in size, scaled to the screen: no user
  // agent or pixel ratio of the device's here.
  const sized = device && fitted && { width: fitted.viewport.w, height: fitted.viewport.h, transform: `scale(${fitted.scale})`, transformOrigin: "0 0" };
  const content =
    url === BLANK ? null : isFrameable(url) ? (
      <iframe
        key={loads}
        ref={frame}
        src={url}
        title={pageLabel(url)}
        sandbox="allow-scripts allow-same-origin allow-forms allow-modals allow-popups allow-downloads"
        className={sized ? "border-0 bg-white" : "size-full border-0 bg-white"}
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
      <AddressBar url={url} state={null} page={page} root={root} field={field} tools={<ToolToggle tool={DEVICE} on={!!choice} onToggle={toggle} />} />
      {choice && device && <DeviceBar choice={choice} device={device} viewport={fitted?.viewport ?? null} dpr={false} onChoice={choose} />}
      <div ref={area} className="relative min-h-0 flex-1 bg-background">
        {device && choice && isFrameable(url) ? (
          fitted &&
          (fitted.fits ? (
            <DeviceFrame device={device} rotated={!!choice.rotated} fitted={fitted} page={pageEl} resize={resize}>
              {content}
            </DeviceFrame>
          ) : (
            <Placeholder title={TOO_SMALL} />
          ))
        ) : (
          content
        )}
      </div>
    </div>
  );
}

interface Tool {
  command: CommandId;
  icon: LucideIcon;
  label: string;
  on: string;
  off: string;
}

const PICK: Tool = { command: "browser.pick", icon: Crosshair, label: "Pick an element", on: "Stop picking (Esc)", off: "Pick an element for the agent" };
const DEVICE: Tool = { command: "browser.toggleDevice", icon: TabletSmartphone, label: "Device mode", on: "Leave device mode", off: "Device mode" };

/** The page's zoom when it isn't 100%; a click sets it back. */
function ZoomChip({ zoom, onReset }: { zoom: number; onReset: () => void }) {
  return (
    <Tip label="Actual Size" shortcut={useShortcut("browser.zoomReset")}>
      <Button type="button" variant="ghost" size="sm" aria-label={`Page zoom ${zoomLabel(zoom)}, reset`} onClick={onReset} className="h-6 px-1.5 font-mono text-[11px] tabular-nums">
        {zoomLabel(zoom)}
      </Button>
    </Tip>
  );
}

const SCHEMES = {
  auto: { icon: SunMoon, label: "Page as the app (click for light)" },
  light: { icon: Sun, label: "Page light (click for dark)" },
  dark: { icon: Moon, label: "Page dark (click for as the app)" },
};

/** Light, dark or as the app, for the page's prefers-color-scheme. */
function SchemeToggle({ scheme, onCycle }: { scheme: Scheme | undefined; onCycle: () => void }) {
  const { icon: Icon, label } = SCHEMES[scheme ?? "auto"];
  return (
    <Tip label={label}>
      <Button type="button" variant="ghost" size="icon-sm" aria-label={label} onClick={onCycle} className={scheme ? "bg-active text-foreground" : undefined}>
        <Icon />
      </Button>
    </Tip>
  );
}

/** One of the page's tools on or off, in the address bar. */
function ToolToggle({ tool, on, onToggle }: { tool: Tool; on: boolean; onToggle: () => void }) {
  const Icon = tool.icon;
  return (
    <Tip label={on ? tool.on : tool.off} shortcut={useShortcut(tool.command)}>
      <Button type="button" variant="ghost" size="icon-sm" aria-pressed={on} aria-label={tool.label} onClick={onToggle} className={on ? "bg-active text-foreground" : undefined}>
        <Icon />
      </Button>
    </Tip>
  );
}
