// Theme + accent. Neutral tokens live in index.css (dark defaults, light
// overrides via [data-theme]); the accent family is owned here because the
// user can change it, and each theme wants different steps of the same ramp.

export type ThemePref = "system" | "light" | "dark";

// Tailwind ramp slices: [200, 400, 500, 600, 700, 900].
export const ACCENTS = {
  teal: ["#99f6e4", "#2dd4bf", "#14b8a6", "#0d9488", "#0f766e", "#134e4a"],
  blue: ["#bfdbfe", "#60a5fa", "#3b82f6", "#2563eb", "#1d4ed8", "#1e3a8a"],
  violet: ["#ddd6fe", "#a78bfa", "#8b5cf6", "#7c3aed", "#6d28d9", "#4c1d95"],
  rose: ["#fecdd3", "#fb7185", "#f43f5e", "#e11d48", "#be123c", "#881337"],
  green: ["#bbf7d0", "#4ade80", "#22c55e", "#16a34a", "#15803d", "#14532d"],
} as const;

export type AccentName = keyof typeof ACCENTS;

const THEME_KEY = "mathom:theme";
const ACCENT_KEY = "mathom:accent";

export function loadThemePref(): ThemePref {
  const v = localStorage.getItem(THEME_KEY);
  return v === "light" || v === "dark" ? v : "system";
}

export function loadAccent(): AccentName {
  const v = localStorage.getItem(ACCENT_KEY);
  return v !== null && v in ACCENTS ? (v as AccentName) : "teal";
}

export function saveThemeSettings(pref: ThemePref, accent: AccentName) {
  localStorage.setItem(THEME_KEY, pref);
  localStorage.setItem(ACCENT_KEY, accent);
}

export function resolvedTheme(
  pref: ThemePref,
  systemDark: boolean,
): "light" | "dark" {
  return pref === "system" ? (systemDark ? "dark" : "light") : pref;
}

/** Swatch color for the accent picker. */
export function accentSwatch(name: AccentName): string {
  return ACCENTS[name][2];
}

export function applyTheme(pref: ThemePref, accent: AccentName) {
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const theme = resolvedTheme(pref, dark);
  const [c200, c400, c500, c600, c700, c900] = ACCENTS[accent];
  const vars =
    theme === "dark"
      ? { solid: c600, hover: c500, ink: c400, edge: c700, soft: c900 }
      : { solid: c600, hover: c700, ink: c600, edge: c500, soft: c200 };

  const el = document.documentElement;
  el.dataset.theme = theme;
  el.style.setProperty("--color-accent", vars.solid);
  el.style.setProperty("--color-accent-hover", vars.hover);
  el.style.setProperty("--color-accent-ink", vars.ink);
  el.style.setProperty("--color-accent-edge", vars.edge);
  el.style.setProperty("--color-accent-soft", vars.soft);
}

/** Called before the first render so the stored theme paints frame one. */
export function applyStoredTheme() {
  applyTheme(loadThemePref(), loadAccent());
}
