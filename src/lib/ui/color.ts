/** A CSS variable of the document root (index.css), as it resolves now. */
export const cssVar = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

// The canvas resolves any CSS color (rgb() with alpha, color-mix(), names) to its pixels.
let probe: CanvasRenderingContext2D | null = null;

/** `color` as #RRGGBBAA; laid `under` another color, as the solid #RRGGBB it shows as. */
export function toHex(color: string, under?: string) {
  probe ??= document.createElement("canvas").getContext("2d", { willReadFrequently: true })!;
  probe.clearRect(0, 0, 1, 1);
  if (under !== undefined) {
    probe.fillStyle = under;
    probe.fillRect(0, 0, 1, 1);
  }
  probe.fillStyle = color;
  probe.fillRect(0, 0, 1, 1);
  const rgba = probe.getImageData(0, 0, 1, 1).data;
  return "#" + [...(under !== undefined ? rgba.slice(0, 3) : rgba)].map((n) => n.toString(16).padStart(2, "0")).join("");
}
