import { useEffect, useState } from "react";
import {
  type AccentName,
  type ThemePref,
  applyTheme,
  loadAccent,
  loadThemePref,
  saveThemeSettings,
} from "../lib/theme";

/** Applies + persists theme/accent, follows the OS under "system"; `themeRev` bumps so canvas renderers re-bake. */
export function useTheme() {
  const [pref, setPref] = useState<ThemePref>(loadThemePref);
  const [accent, setAccent] = useState<AccentName>(loadAccent);
  const [themeRev, setThemeRev] = useState(0);

  useEffect(() => {
    applyTheme(pref, accent);
    saveThemeSettings(pref, accent);
    setThemeRev((r) => r + 1);
    if (pref !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      applyTheme(pref, accent);
      setThemeRev((r) => r + 1);
    };
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [pref, accent]);

  return { pref, setPref, accent, setAccent, themeRev };
}
