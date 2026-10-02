// Obsidian's default theme colors, as RGB channels ("r g b") for dark and light themes: the
// callouts and the canvas's six colors use them.
export type Rgb = readonly [dark: string, light: string];

export const COLORS = {
  red: ["251 70 76", "233 49 71"],
  orange: ["233 151 63", "236 117 0"],
  yellow: ["224 222 113", "224 172 0"],
  green: ["68 207 110", "8 185 78"],
  cyan: ["83 223 221", "0 191 188"],
  blue: ["2 122 255", "8 109 221"],
  purple: ["168 130 255", "120 82 238"],
  gray: ["158 158 158", "158 158 158"],
} as const satisfies Record<string, Rgb>;
