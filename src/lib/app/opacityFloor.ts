const rgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
const luminance = (c: number[]) => {
  const [r, g, b] = c.map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
/** WCAG's contrast ratio of two sRGB colors (0-1 channels). */
export function contrast(a: number[], b: number[]) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/**
 * The lowest window opacity (percent, on `range`'s steps) at which a theme's `foreground` text on
 * its `background` keeps 3:1 over a black and a white desktop alike, the darkest and lightest a
 * blurred wallpaper averages to. Composited on the encoded values, as WebKit does. `range.min`
 * when the colors aren't #RRGGBB.
 */
export function opacityFloor(foreground: string, background: string, range: { min: number; max: number; step: number }) {
  const hex = /^#[0-9a-f]{6}$/i;
  if (!hex.test(foreground) || !hex.test(background)) return range.min;
  const [text, under] = [rgb(foreground), rgb(background)];
  const readable = (pct: number) =>
    [0, 1].every((desk) => contrast(text, under.map((v) => (v * pct) / 100 + desk * (1 - pct / 100))) >= 3);
  let pct = range.min;
  while (pct < range.max && !readable(pct)) pct += range.step;
  return pct;
}
