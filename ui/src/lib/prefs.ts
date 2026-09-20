// View preferences that outlive a scan. Values the back end also has to agree
// on are mirrored here, same as palette.ts mirrors mathom-core's categories.

/** Mirrors `MAX_TREEMAP_DEPTH` in src-tauri/src/scan.rs. */
export const MAX_TREEMAP_DEPTH = 24;

/**
 * How deep the treemap expands, as the Depth setting offers it.
 *
 * `"auto"` is the map's own judgement — a directory opens when what is inside
 * it would be legible, and folds when it would not. The rest are the original
 * fixed levels: take every level the cap allows and ask nothing, which is a
 * different picture and a legitimate thing to want.
 */
export type DepthPref = "auto" | "all" | 1 | 2 | 3;

const DEPTH_KEY = "mathom:treemapDepth";

/** The fixed levels the settings menu offers. */
const CAPS = [1, 2, 3] as const;

export const DEPTH_OPTIONS: { value: DepthPref; label: string }[] = [
  { value: "auto", label: "Auto" },
  { value: "all", label: "All" },
  ...CAPS.map((depth) => ({ value: depth as DepthPref, label: String(depth) })),
];

/**
 * Total, and its domain is exactly the options above: a stored value no button
 * can show would leave the control with nothing selected, so anything else
 * reads as Auto.
 */
export function parseDepth(raw: string | null): DepthPref {
  if (raw === "all") return "all";
  const n = Number(raw);
  return (CAPS as readonly number[]).includes(n) ? (n as DepthPref) : "auto";
}

/**
 * The depth the back end should lay out to — `null` for Auto, which is the
 * only value that leaves the legibility rules in charge.
 */
export function layoutDepth(pref: DepthPref): number | null {
  if (pref === "auto") return null;
  if (pref === "all") return MAX_TREEMAP_DEPTH;
  return Math.min(Math.max(pref, 1), MAX_TREEMAP_DEPTH);
}

export function loadTreemapDepth(): DepthPref {
  return parseDepth(localStorage.getItem(DEPTH_KEY));
}

export function saveTreemapDepth(depth: DepthPref) {
  localStorage.setItem(DEPTH_KEY, String(depth));
}
