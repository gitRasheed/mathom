// Indexed by mathom-core's `Category as u8`. Index 0 (directories) is
// theme-dependent — canvas code paints plates with canvasColors().plate.
export const PALETTE: readonly string[] = [
  "#30323a", // 0 directory plate (dark fallback; see canvasColors)
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
    background: v("--color-app"),
    plate: v("--color-plate"),
    selection: v("--color-ink"),
    hoverRing: v("--color-accent-ink"),
  };
}
