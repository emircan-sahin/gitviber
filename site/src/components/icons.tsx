import { useId, type SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement>;

const stroke = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.75,
  strokeLinecap: "round",
  strokeLinejoin: "round",
  "aria-hidden": true,
} as const;

/** assets/icon.svg, with ids unique per instance so two logos on a page don't share gradients. */
export function Logo(props: IconProps) {
  const id = useId();
  return (
    <svg viewBox="100 100 824 824" aria-hidden {...props}>
      <defs>
        <linearGradient id={`${id}bg`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#26241f" />
          <stop offset="1" stopColor="#141311" />
        </linearGradient>
        <linearGradient id={`${id}ac`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#5eead4" />
          <stop offset="1" stopColor="#2dd4bf" />
        </linearGradient>
      </defs>
      <rect x="100" y="100" width="824" height="824" rx="190" fill={`url(#${id}bg)`} />
      <rect x="100.5" y="100.5" width="823" height="823" rx="190" fill="none" stroke="#3a3731" strokeWidth="3" />
      <g fill="none" stroke={`url(#${id}ac)`} strokeWidth="56" strokeLinecap="round">
        <path d="M380 300 V724" />
        <path d="M644 380 C644 520 380 500 380 620" />
      </g>
      <circle cx="380" cy="300" r="62" fill="#141311" stroke={`url(#${id}ac)`} strokeWidth="44" />
      <circle cx="380" cy="724" r="62" fill="#141311" stroke={`url(#${id}ac)`} strokeWidth="44" />
      <circle cx="644" cy="340" r="62" fill="#fbbf24" />
    </svg>
  );
}

export const GitHubIcon = (props: IconProps) => (
  <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden {...props}>
    <path d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z" />
  </svg>
);

export const DownloadIcon = (props: IconProps) => (
  <svg viewBox="0 0 24 24" {...stroke} {...props}>
    <path d="M12 4v11m0 0-4.5-4.5M12 15l4.5-4.5M5 19.5h14" />
  </svg>
);

export const CopyIcon = (props: IconProps) => (
  <svg viewBox="0 0 24 24" {...stroke} {...props}>
    <rect x="8.5" y="8.5" width="11" height="11" rx="2.5" />
    <path d="M15.5 5.5A2 2 0 0 0 13.5 4H6a2 2 0 0 0-2 2v7.5a2 2 0 0 0 1.5 1.94" />
  </svg>
);

export const CheckIcon = (props: IconProps) => (
  <svg viewBox="0 0 24 24" {...stroke} {...props}>
    <path d="m5 12.5 4.5 4.5L19 7.5" />
  </svg>
);
