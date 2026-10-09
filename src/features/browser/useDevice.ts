import { useCallback } from "react";
import { DEFAULT_DEVICE, type DeviceChoice, deviceOf, isDeviceChoice } from "@/lib/browser/devices";
import type { Selection } from "@/lib/repo/selection";
import { readJson, writeJson } from "@/lib/storage";

type BrowserSelection = Extract<Selection, { kind: "browser" }>;

// The device last shown, for device mode's next toggle in any tab.
const LAST_KEY = "gitviber.browserDevice";

/** A tab's device mode: the device it shows (kept with the tab), picked or toggled. */
export function useDevice(tabKey: string, sel: BrowserSelection, onUpdate: (key: string, sel: Selection) => void) {
  // A name no longer in devices.json shows the page as itself.
  const choice = sel.device && deviceOf(sel.device) ? sel.device : null;
  const choose = useCallback(
    (next: DeviceChoice | null) => {
      if (next) writeJson(LAST_KEY, next);
      const { device: _, ...rest } = sel;
      onUpdate(tabKey, next ? { ...rest, device: next } : rest);
    },
    [sel, tabKey, onUpdate],
  );
  const toggle = () => choose(choice ? null : readJson<DeviceChoice>(LAST_KEY, DEFAULT_DEVICE, isDeviceChoice));
  return { choice, choose, toggle };
}
