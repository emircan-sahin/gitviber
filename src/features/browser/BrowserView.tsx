import { Crosshair, type LucideIcon, TabletSmartphone } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { browserApi, type BrowserGo, errorMessage, github } from "@/lib/api";
import { failed, toast } from "@/lib/app/toast";
import { onBrowserKey, setBrowserState, useBrowserState, useParks } from "@/lib/browser/store";
import { RESPONSIVE } from "@/lib/browser/devices";
import { BLANK, isFrameable, pageLabel } from "@/lib/browser/url";
import { type CommandId, commandIn, useCommands, useShortcut } from "@/lib/commands/keybindings";
import { IS_LINUX } from "@/lib/platform";
import { useSettings } from "@/lib/settings";
import type { Selection } from "@/lib/repo/selection";
import { Placeholder } from "@/features/viewer/FileHeader";
import { AddressBar, type PageControl } from "./AddressBar";
import { ConsoleBadge, ConsolePanel } from "./ConsolePanel";
import { PickNote } from "./PickNote";
import { DeviceBar } from "./DeviceBar";
import { DeviceFrame } from "./DeviceFrame";
import { useDevice } from "./useDevice";
import { useDeviceFit } from "./useDeviceFit";
import { useNativeRect } from "./useNativeRect";
import { usePicker } from "./usePicker";

type BrowserSelection = Extract<Selection, { kind: "browser" }>;

interface Props {
  tabKey: string;
  sel: BrowserSelection;
  root: string;
  onUpdate: (key: string, sel: Selection) => void;
}

/** The tab's own keys, in its address bar or its page; the GO ones go to the page. */
const GO = { "browser.reload": "reload", "browser.back": "back", "browser.forward": "forward" } as const satisfies Record<string, BrowserGo>;
const OWN = ["browser.focusAddress", "browser.inspect", "browser.toggleDevice", "browser.pick", ...(Object.keys(GO) as (keyof typeof GO)[])] as const;

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
  const capture = useSettings().browserConsole;
  const [consoleOpen, setConsoleOpen] = useState(false);
  const { picking, pick, toggle: togglePick, closeNote } = usePicker(id);

  // On the first show and after each park, on its own id: where it loads next is the page's business.
  useEffect(() => {
    let live = true;
    setMade(false);
    browserApi.create(id, root, sel.url, ua).then(
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
  useCommands({ "browser.inspect": made ? inspect : undefined, "browser.toggleDevice": made ? toggle : undefined, "browser.pick": made ? togglePick : undefined });

  // The page's keys arrive through onBrowserKey, registered once: the latest toggles are read here.
  const toggleRef = useRef(toggle);
  toggleRef.current = toggle;
  const pickRef = useRef(togglePick);
  pickRef.current = togglePick;
  /** One of the tab's own keys, run; false for any other key. */
  const runOwn = (e: KeyboardEvent) => {
    const command = commandIn(OWN, e);
    if (!command) return false;
    if (command === "browser.inspect") inspect();
    else if (command === "browser.toggleDevice") toggleRef.current();
    else if (command === "browser.pick") pickRef.current();
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
  const image = picture && <img src={picture} alt="" className="absolute inset-0 size-full object-cover select-none" />;
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
            <ToolToggle tool={DEVICE} on={!!choice} onToggle={toggle} />
          </>
        }
      />
      {choice && device && <DeviceBar choice={choice} device={device} viewport={fitted?.viewport ?? null} dpr={dpr} onChoice={choose} />}
      {pick && <PickNote pick={pick} device={device && fitted && { name: device.name, viewport: fitted.viewport }} root={root} onClose={closeNote} />}
      {capture && consoleOpen && <ConsolePanel id={id} root={root} onClose={() => setConsoleOpen(false)} />}
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
