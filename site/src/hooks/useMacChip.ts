import { useEffect, useState } from "react";
import { usePlatform } from "./usePlatform.ts";

export type Chip = "arm" | "intel";

interface ClientHints {
  getHighEntropyValues(hints: string[]): Promise<{ architecture?: string }>;
}

// Safari and Firefox call every Mac "Intel Mac OS X", so the user agent can't tell. Chromium's
// client hints can; elsewhere a GPU no Apple Silicon Mac has gives an Intel one away. Anything
// unsure gets Apple Silicon.
async function detect(): Promise<Chip> {
  const hints = (navigator as Navigator & { userAgentData?: ClientHints }).userAgentData;
  const arch = await hints?.getHighEntropyValues(["architecture"]).then(
    (v) => v.architecture,
    () => undefined,
  );
  if (arch) return arch === "x86" ? "intel" : "arm";
  const gl = document.createElement("canvas").getContext("webgl");
  if (!gl) return "arm";
  // Firefox names the GPU here; WebKit says "WebKit WebGL" and keeps the name in the extension.
  let gpu = String(gl.getParameter(gl.RENDERER));
  const info = gpu.startsWith("WebKit") ? gl.getExtension("WEBGL_debug_renderer_info") : null;
  if (info) gpu = String(gl.getParameter(info.UNMASKED_RENDERER_WEBGL));
  gl.getExtension("WEBGL_lose_context")?.loseContext();
  return /Intel|AMD|Radeon|NVIDIA|GeForce/i.test(gpu) ? "intel" : "arm";
}

let detected: Promise<Chip> | undefined;

/** The visitor's Mac's chip, for picking its .dmg: Apple Silicon until the browser says Intel. */
export function useMacChip(): Chip {
  const mac = usePlatform() === "mac";
  const [chip, setChip] = useState<Chip>("arm");
  useEffect(() => {
    if (!mac) return;
    let live = true;
    detected ??= detect().catch((): Chip => "arm");
    void detected.then((c) => live && setChip(c));
    return () => {
      live = false;
    };
  }, [mac]);
  return chip;
}
