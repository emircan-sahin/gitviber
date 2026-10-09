import { ChevronDown, RotateCcwSquare } from "lucide-react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Tip } from "@/components/ui/tooltip";
import { type Device, type DeviceChoice, DEVICES, RESPONSIVE, RESPONSIVE_MAX, RESPONSIVE_MIN } from "@/lib/browser/devices";
import { cn } from "@/lib/utils";

/** What showing a page as a device here doesn't do, as a desktop browser's device mode doesn't. */
const LIMITS = "Not quite the device: no touch events, <meta name=viewport> is ignored, hover still works, and env(safe-area-inset-*) is 0.";

const control = "flex h-6 items-center gap-1 rounded-md px-1.5 text-[11.5px] text-muted-foreground outline-none hover:bg-hover focus-visible:bg-hover hover:text-foreground focus-visible:text-foreground";

/**
 * Device mode's bar under the address: the device, turned or not, and its size; Responsive's
 * size typed in. `dpr`: false where this macOS can't set the device's pixel ratio.
 */
export function DeviceBar({ choice, device, dpr, onChoice }: { choice: DeviceChoice; device: Device; dpr: boolean; onChoice: (next: DeviceChoice) => void }) {
  const responsive = choice.name === RESPONSIVE;
  const [w, h] = choice.rotated ? [device.h, device.w] : [device.w, device.h];
  const size = (side: "w" | "h", n: number) => onChoice({ ...choice, w: device.w, h: device.h, [side]: n });
  return (
    <div className="flex h-8 shrink-0 items-center gap-1 border-b border-border px-2">
      <DropdownMenu>
        <Tip label={LIMITS}>
          <DropdownMenuTrigger asChild>
            <button type="button" className={cn(control, "text-foreground")}>
              {device.name}
              <ChevronDown className="size-3" />
            </button>
          </DropdownMenuTrigger>
        </Tip>
        <DropdownMenuContent align="start">
          <DropdownMenuRadioGroup value={choice.name} onValueChange={(name) => onChoice(name === RESPONSIVE ? { name, w: device.w, h: device.h } : { name, rotated: choice.rotated })}>
            <DropdownMenuRadioItem value={RESPONSIVE}>{RESPONSIVE}</DropdownMenuRadioItem>
            <DropdownMenuSeparator />
            {DEVICES.map((d) => (
              <DropdownMenuRadioItem key={d.name} value={d.name}>
                {d.name}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
      {!responsive && (
        <Tip label="Rotate">
          <button type="button" aria-label="Rotate" className={control} onClick={() => onChoice({ ...choice, rotated: !choice.rotated })}>
            <RotateCcwSquare className="size-3.5" />
          </button>
        </Tip>
      )}
      {responsive ? (
        <div className="flex items-center gap-1 font-mono text-[11.5px] text-muted-foreground">
          <SizeInput label="Width" value={device.w} onChange={(n) => size("w", n)} />×
          <SizeInput label="Height" value={device.h} onChange={(n) => size("h", n)} />
        </div>
      ) : (
        <span className="font-mono text-[11.5px] text-subtle">
          {w}×{h}
          {device.dpr > 0 && ` @${device.dpr}x`}
        </span>
      )}
      {!dpr && device.dpr > 0 && (
        <Tip label="The page sees this screen's pixel ratio here, not the device's.">
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
    if (Number.isFinite(n) && el.value.trim()) onChange(Math.round(Math.min(RESPONSIVE_MAX, Math.max(RESPONSIVE_MIN, n))));
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
