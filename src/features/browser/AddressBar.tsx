import { ArrowLeft, ArrowRight, ExternalLink, RotateCw, ShieldAlert, X } from "lucide-react";
import { type FormEvent, type RefObject, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { DisabledTip, Tip } from "@/components/ui/tooltip";
import { browserApi, type BrowserGo, type BrowserState, github } from "@/lib/api";
import { failed } from "@/lib/app/toast";
import { BLANK, normalizeUrl } from "@/lib/browser/url";
import { useShortcut } from "@/lib/commands/keybindings";
import { cn } from "@/lib/utils";

interface Props {
  id: string;
  /** Where the tab is, while its page hasn't said yet. */
  url: string;
  state: BrowserState | null;
  /** The address field, for ⌘L. */
  field: RefObject<HTMLInputElement | null>;
}

/** Back, forward, reload, the address (a port, a host or a URL; Enter loads it), and the system browser. */
export function AddressBar({ id, url, state, field }: Props) {
  const current = state?.url || url;
  // What's typed while the field has focus; otherwise it shows where the page is.
  const [typed, setTyped] = useState<string | null>(null);
  const [invalid, setInvalid] = useState(false);
  const backKey = useShortcut("browser.back");
  const forwardKey = useShortcut("browser.forward");
  const reloadKey = useShortcut("browser.reload");
  const go = (to: BrowserGo) => void browserApi.go(id, to).catch(failed("The page didn't respond"));
  const toPage = () => {
    setTyped(null);
    field.current?.blur();
    void browserApi.focus(id, true).catch(() => {});
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const next = normalizeUrl(typed ?? current);
    if (!next) return setInvalid(true);
    toPage();
    void browserApi.navigate(id, next).catch(failed("Could not open the page"));
  };

  return (
    <form onSubmit={submit} className="relative flex h-9 shrink-0 items-center gap-0.5 border-b border-border px-1.5">
      <DisabledTip label="Back" shortcut={backKey} disabled={!state?.canBack}>
        <Button type="button" variant="ghost" size="icon-sm" disabled={!state?.canBack} onClick={() => go("back")}>
          <ArrowLeft />
        </Button>
      </DisabledTip>
      <DisabledTip label="Forward" shortcut={forwardKey} disabled={!state?.canForward}>
        <Button type="button" variant="ghost" size="icon-sm" disabled={!state?.canForward} onClick={() => go("forward")}>
          <ArrowRight />
        </Button>
      </DisabledTip>
      {state?.loading ? (
        <Tip label="Stop">
          <Button type="button" variant="ghost" size="icon-sm" onClick={() => go("stop")}>
            <X />
          </Button>
        </Tip>
      ) : (
        <Tip label="Reload" shortcut={reloadKey}>
          <Button type="button" variant="ghost" size="icon-sm" onClick={(e) => go(e.shiftKey ? "hardReload" : "reload")}>
            <RotateCw />
          </Button>
        </Tip>
      )}
      <Input
        ref={field}
        aria-label="Address"
        aria-invalid={invalid}
        placeholder="A port (5173) or an address"
        spellCheck={false}
        autoCapitalize="off"
        autoCorrect="off"
        value={typed ?? (current === BLANK ? "" : current)}
        onFocus={(e) => {
          setTyped(e.currentTarget.value);
          e.currentTarget.select();
        }}
        onChange={(e) => {
          setTyped(e.currentTarget.value);
          setInvalid(false);
        }}
        onBlur={() => {
          setTyped(null);
          setInvalid(false);
        }}
        onKeyDown={(e) => {
          if (e.key !== "Escape") return;
          e.preventDefault();
          toPage();
        }}
        className={cn("mx-1 h-6 min-w-0 flex-1 font-mono text-[11.5px]", invalid && "border-destructive focus:border-destructive")}
      />
      {invalid && <span className="mr-1 shrink-0 text-[11.5px] text-destructive">Not an address</span>}
      {state?.insecure && (
        <Tip label="Its self-signed certificate is let through on this machine only">
          <span tabIndex={0} className="flex size-6 shrink-0 items-center justify-center rounded-md text-modified outline-none focus-visible:ring-1 focus-visible:ring-ring">
            <ShieldAlert className="size-3.5" />
          </span>
        </Tip>
      )}
      <DisabledTip label="Open in Browser" disabled={current === BLANK}>
        <Button type="button" variant="ghost" size="icon-sm" disabled={current === BLANK} onClick={() => void github.openUrl(current).catch(failed("Could not open the link"))}>
          <ExternalLink />
        </Button>
      </DisabledTip>
      {state?.loading && <span aria-hidden className="absolute bottom-0 left-0 h-px bg-primary transition-[width]" style={{ width: `${Math.max(state.progress, 0.05) * 100}%` }} />}
    </form>
  );
}
