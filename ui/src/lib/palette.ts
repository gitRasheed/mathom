/**
 * The colour of a folder block. The one category colour that is not a file
 * type: a folder is a container, and a plate painted in the app's own
 * background grey made an unopened folder look like nothing was there at all.
 * Its hue is not any file colour's, so a container is never mistaken for what
 * it holds.
 *
 * Picked for being a *large* area rather than a small one — folders cover more
 * of the map than anything else, so this is the map's tone. It sits clearly
 * above the backdrop in value, which is what makes a block read as raised
 * instead of as a slightly different patch of background.
 *
 * It is a colour and nothing else — folders are drawn exactly like files.
 */
export const FOLDER_PLATE = "#55618f";

// Indexed by mathom-core's `Category as u8`.
export const PALETTE: readonly string[] = [
  FOLDER_PLATE, // 0 directory
  "#a855f7", // 1 video
  "#22c55e", // 2 audio
  "#eab308", // 3 image
  "#f97316", // 4 archive
  "#3b82f6", // 5 document
  "#06b6d4", // 6 code
  "#ec4899", // 7 executable
  "#64748b", // 8 system
  "#84cc16", // 9 data
  "#71717a", // 10 other
];

/** Canvas colors resolved from the active theme's CSS variables. */
export function canvasColors() {
  const style = getComputedStyle(document.documentElement);
  const v = (name: string) => style.getPropertyValue(name).trim();
  return {
    /** The map's surface, behind every block — not the window background. */
    plate: v("--color-plate"),
    selection: v("--color-ink"),
    hoverRing: v("--color-accent-ink"),
  };
}

/** `#rrggbb` or `rgb(...)`. */
function rgb(color: string): [number, number, number] {
  if (color.startsWith("#")) {
    const n = Number.parseInt(color.slice(1), 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }
  const parts = color.match(/\d+/g);
  return parts
    ? [Number(parts[0]), Number(parts[1]), Number(parts[2])]
    : [0, 0, 0];
}

function luminance(r: number, g: number, b: number): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/**
 * Ink or paper for text on `fill`, used by the optional block labels. File
 * blocks keep the fixed category colours whatever the theme, so this picks
 * literal near-black or near-white rather than a theme token — a token would
 * be chosen for the plate, not for the colour actually underneath.
 */
export function textOn(fill: string): string {
  const [r, g, b] = rgb(fill);
  return luminance(r, g, b) > 0.45
    ? "rgba(12, 12, 14, 0.88)"
    : "rgba(250, 250, 250, 0.92)";
}
