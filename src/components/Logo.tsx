import { useId } from "react";
import { cn } from "@/lib/utils";

/**
 * The app icon (public/icon.svg) inline, so the splash can draw its branch in.
 * Gradient ids come from useId: several logos can be on screen at once. icon.svg's vertical
 * line never renders (a bounding-box gradient on a zero-width path paints nothing) and the
 * shipped PNG icons were rasterized without it, so it's left out here to match them.
 */
export function Logo({ className, animate }: { className?: string; animate?: boolean }) {
  const id = useId();
  const bg = `${id}bg`;
  const ac = `${id}ac`;
  return (
    <svg viewBox="100 100 824 824" className={cn("shrink-0", animate && "logo-draw", className)} aria-hidden>
      <defs>
        <linearGradient id={bg} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#26241f" />
          <stop offset="1" stopColor="#141311" />
        </linearGradient>
        <linearGradient id={ac} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#5eead4" />
          <stop offset="1" stopColor="#2dd4bf" />
        </linearGradient>
      </defs>
      <rect x="100" y="100" width="824" height="824" rx="190" fill={`url(#${bg})`} />
      <rect x="100.5" y="100.5" width="823" height="823" rx="190" fill="none" stroke="#3a3731" strokeWidth="3" />
      <g fill="none" stroke={`url(#${ac})`} strokeWidth="56" strokeLinecap="round">
        <path className="logo-line" pathLength={1} d="M380 620 C380 500 644 520 644 380" />
      </g>
      <circle className="logo-dot" cx="380" cy="300" r="62" fill="#141311" stroke={`url(#${ac})`} strokeWidth="44" />
      <circle className="logo-dot" cx="380" cy="724" r="62" fill="#141311" stroke={`url(#${ac})`} strokeWidth="44" />
      <circle className="logo-dot logo-dot-head" cx="644" cy="340" r="62" fill="#fbbf24" />
    </svg>
  );
}

/** Logo and name, for the title bar: screenshots and recordings should say which app this is. */
export function Wordmark() {
  return (
    <span className="flex shrink-0 items-center gap-1.5 select-none" data-tauri-drag-region>
      <Logo className="pointer-events-none size-[18px]" />
      <span className="pointer-events-none text-[12.5px] font-semibold tracking-tight">GitViber</span>
    </span>
  );
}
