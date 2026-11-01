const STYLE_KEY = "mathom:treemap-style";
const DEPTH_KEY = "mathom:treemap-depth";

/** Mirrors mathom-core's MAX_LAYOUT_DEPTH. */
export const MAX_TREEMAP_DEPTH = 24;

/** Default zooms into a clicked folder; SpaceSniffer opens it in place. */
export type TreemapStyle = "default" | "spacesniffer";

export interface TreemapPrefs {
  style: TreemapStyle;
  /** null draws every level. */
  depth: number | null;
}

/** SpaceSniffer needs something folded to open; a stored All is kept for Default. */
export function treemapDepth(prefs: TreemapPrefs): number | null {
  return prefs.style === "spacesniffer" ? (prefs.depth ?? 1) : prefs.depth;
}

export function loadTreemapPrefs(): TreemapPrefs {
  const n = Number(localStorage.getItem(DEPTH_KEY));
  return {
    style:
      localStorage.getItem(STYLE_KEY) === "spacesniffer"
        ? "spacesniffer"
        : "default",
    depth: Number.isInteger(n) && n >= 1 && n <= MAX_TREEMAP_DEPTH ? n : null,
  };
}

export function saveTreemapPrefs(prefs: TreemapPrefs) {
  localStorage.setItem(STYLE_KEY, prefs.style);
  localStorage.setItem(DEPTH_KEY, String(prefs.depth ?? "all"));
}
