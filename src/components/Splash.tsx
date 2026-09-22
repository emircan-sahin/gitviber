import { useEffect, useState } from "react";
import { Logo } from "./Logo";

/** Long enough for the logo to finish drawing, so a fast launch doesn't just flash it. */
const MIN_MS = 1100;

/**
 * Launch screen over the app while the last repository reopens. The workspace mounts and
 * loads underneath; once `ready` and the logo has drawn, this fades out and unmounts.
 */
export function Splash({ ready }: { ready: boolean }) {
  const [shown, setShown] = useState(false);
  const [gone, setGone] = useState(false);
  useEffect(() => {
    const t = setTimeout(() => setShown(true), MIN_MS);
    return () => clearTimeout(t);
  }, []);
  const leaving = ready && shown;
  if (gone) return null;
  return (
    <div
      data-tauri-drag-region
      onTransitionEnd={() => leaving && setGone(true)}
      className={`fixed inset-0 z-50 flex flex-col items-center justify-center gap-4 bg-background transition-opacity duration-300 ${leaving ? "pointer-events-none opacity-0" : ""}`}
    >
      <Logo animate className="pointer-events-none size-24" />
      <div className="splash-text pointer-events-none text-center">
        <div className="text-[22px] font-semibold tracking-tight">GitViber</div>
        <div className="mt-1 text-[12px] text-muted-foreground">Review what your agent wrote.</div>
      </div>
    </div>
  );
}
