import { ChevronDown, RotateCcwSquare } from "lucide-react";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Tip } from "@/components/ui/tooltip";
import { clampSide, type Device, type DeviceChoice, DEVICES, RESPONSIVE, RESPONSIVE_MAX, RESPONSIVE_MIN } from "@/lib/browser/devices";
import { IS_LINUX } from "@/lib/platform";

/** What showing a page as a device here doesn't do, as a desktop browser's device mode doesn't. */
const LIMITS = "Not quite the device: no touch events, <meta name=viewport> is ignored, hover still works, and env(safe-area-inset-*) is 0.";

/** Where the page can't be told it's the device beyond its size. */
const NO_DPR = IS_LINUX ? "On Linux the page sees this screen's pixel ratio and the app's user agent, not the device's." : "The page sees this screen's pixel ratio here, not the device's.";

/**
 * Device mode's bar under the address: the device, turned or not, and the page's viewport;
 * Responsive's size typed in. `dpr`: false where the page can't be given the device's pixel ratio.
 */
export function DeviceBar({ choice, device, viewport, dpr, onChoice }: { choice: DeviceChoice; device: Device; viewport: { w: number; h: number } | null; dpr: boolean; onChoice: (next: DeviceChoice) => void }) {
  const responsive = choice.name === RESPONSIVE;
  const size = (side: "w" | "h", n: number) => onChoice({ ...choice, w: device.w, h: device.h, [side]: n });
  return (
    <div className="flex h-8 shrink-0 items-center gap-1 border-b border-border px-2">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button type="button" variant="ghost" size="sm" className="text-foreground">
            {device.name}
            <ChevronDown className="size-3" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="max-w-72">
          <DropdownMenuRadioGroup value={choice.name} onValueChange={(name) => onChoice(name === RESPONSIVE ? { name, w: device.w, h: device.h } : { name, rotated: choice.rotated })}>
            <DropdownMenuRadioItem value={RESPONSIVE}>{RESPONSIVE}</DropdownMenuRadioItem>
            <DropdownMenuSeparator />
            {DEVICES.map((d) => (
              <DropdownMenuRadioItem key={d.name} value={d.name}>
                {d.name}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
          <DropdownMenuSeparator />
          {/* Here, not in a tooltip: one on the trigger came back as the menu gave it focus again. */}
          <p className="px-2 py-1.5 text-[11px] leading-relaxed text-subtle">{LIMITS}</p>
        </DropdownMenuContent>
      </DropdownMenu>
      {!responsive && (
        <Tip label="Rotate">
          <Button type="button" variant="ghost" size="icon-sm" aria-label="Rotate" onClick={() => onChoice({ ...choice, rotated: !choice.rotated })}>
            <RotateCcwSquare />
          </Button>
        </Tip>
      )}
      {responsive && (
        <div className="flex items-center gap-1 font-mono text-[11.5px] text-muted-foreground">
          <SizeInput label="Width" value={device.w} onChange={(n) => size("w", n)} />×
          <SizeInput label="Height" value={device.h} onChange={(n) => size("h", n)} />
        </div>
      )}
      {viewport && (
        <Tip label="The page's viewport: the screen less its status bar and home indicator">
          <span tabIndex={0} className="rounded-sm px-1 font-mono text-[11.5px] text-subtle outline-none focus-visible:ring-1 focus-visible:ring-ring">
            {viewport.w}×{viewport.h}
            {device.dpr > 0 && ` @${device.dpr}x`}
          </span>
        </Tip>
      )}
      {!dpr && device.dpr > 0 && (
        <Tip label={NO_DPR}>
          <span tabIndex={0} className="ml-1 rounded-sm border border-border px-1 font-mono text-[10px] text-subtle outline-none focus-visible:ring-1 focus-visible:ring-ring">
            DPR n/a
          </span>
        </Tip>
      )}
    </div>
  );
}

/** A side typed in: taken on Enter or leaving the field, so "3" on the way to 320 isn't 200. */
function SizeInput({ label, value, onChange }: { label: string; value: number; onChange: (n: number) => void }) {
  const take = (el: HTMLInputElement) => {
    const n = Number(el.value);
    if (Number.isFinite(n) && el.value.trim()) onChange(clampSide(n));
    else el.value = String(value);
  };
  return (
    <Input
      // Each new size (a drag) starts the field over.
      key={value}
      aria-label={label}
      type="number"
      min={RESPONSIVE_MIN}
      max={RESPONSIVE_MAX}
      defaultValue={value}
      onBlur={(e) => take(e.currentTarget)}
      onKeyDown={(e) => e.key === "Enter" && take(e.currentTarget)}
      className="h-6 w-16 px-1.5 font-mono text-[11.5px]"
    />
  );
}
