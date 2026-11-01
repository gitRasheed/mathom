import { useEffect, useRef, useState } from "react";
import {
  ACCENTS,
  type AccentName,
  type ThemePref,
  accentSwatch,
} from "../lib/theme";
import {
  MAX_TREEMAP_DEPTH,
  type TreemapPrefs,
  type TreemapStyle,
  treemapDepth,
} from "../lib/prefs";
import { DepthPicker } from "./DepthPicker";
import { PaletteIcon } from "./icons";

const DEPTH_PRESETS = {
  default: [null, 1, 2],
  spacesniffer: [1, 2],
} as const;

const STYLE_OPTIONS: { value: TreemapStyle; label: string; title: string }[] = [
  { value: "default", label: "Default", title: "Click zooms into a folder" },
  {
    value: "spacesniffer",
    label: "SpaceSniffer",
    title: "Click opens a folder in place",
  },
];

const THEME_OPTIONS: { value: ThemePref; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

interface SettingsMenuProps {
  hideSystem: boolean;
  treemapPrefs: TreemapPrefs;
  themePref: ThemePref;
  accent: AccentName;
  onToggleHideSystem: () => void;
  onTreemapPrefs: (prefs: TreemapPrefs) => void;
  onThemePref: (pref: ThemePref) => void;
  onAccent: (accent: AccentName) => void;
}

export function SettingsMenu({
  hideSystem,
  treemapPrefs,
  themePref,
  accent,
  onToggleHideSystem,
  onTreemapPrefs,
  onThemePref,
  onAccent,
}: SettingsMenuProps) {
  const [open, setOpen] = useState(false);
  const depth = treemapDepth(treemapPrefs);
  const [customDepth, setCustomDepth] = useState(
    depth !== null && depth > 2 ? depth : 3,
  );
  const boxRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!boxRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div ref={boxRef} className="relative shrink-0">
      <button
        onClick={() => setOpen((v) => !v)}
        title="Appearance"
        aria-label="Appearance"
        className={`ml-1 flex h-8 w-8 items-center justify-center rounded-md border ${
          open
            ? "border-edge-strong bg-raised text-ink"
            : "border-edge bg-panel text-ink-4 hover:bg-raised hover:text-ink-2"
        }`}
      >
        <PaletteIcon />
      </button>
      {open && (
        <div className="absolute top-9 right-0 z-50 w-52 rounded-md border border-edge-strong bg-panel p-3 shadow-xl">
          <div className="text-[11px] font-medium tracking-wide text-ink-4 uppercase">
            View
          </div>
          <label
            className="mt-1.5 flex cursor-pointer items-center gap-2 text-[12px] text-ink-2"
            title="Hide OS/system files (pagefile, hiberfil, System Volume Information, …)"
          >
            <input
              type="checkbox"
              className="accent-accent"
              checked={hideSystem}
              onChange={onToggleHideSystem}
            />
            Hide system files
          </label>
          <div className="mt-2.5 text-[12px] text-ink-2">Treemap style</div>
          <div className="mt-1.5 flex rounded-md border border-edge p-0.5">
            {STYLE_OPTIONS.map((opt) => (
              <button
                key={opt.value}
                title={opt.title}
                onClick={() =>
                  onTreemapPrefs({ ...treemapPrefs, style: opt.value })
                }
                className={`h-6 flex-1 rounded text-[12px] ${
                  treemapPrefs.style === opt.value
                    ? "bg-raised text-ink"
                    : "text-ink-4 hover:text-ink-2"
                }`}
              >
                {opt.label}
              </button>
            ))}
          </div>
          <div className="mt-2.5 text-[12px] text-ink-2">Treemap depth</div>
          <DepthPicker
            value={depth}
            presets={DEPTH_PRESETS[treemapPrefs.style]}
            custom={customDepth}
            max={MAX_TREEMAP_DEPTH}
            onChange={(d) => onTreemapPrefs({ ...treemapPrefs, depth: d })}
            onCustom={setCustomDepth}
          />
          <div className="mt-3 text-[11px] font-medium tracking-wide text-ink-4 uppercase">
            Theme
          </div>
          <div className="mt-1.5 flex rounded-md border border-edge p-0.5">
            {THEME_OPTIONS.map((opt) => (
              <button
                key={opt.value}
                onClick={() => onThemePref(opt.value)}
                className={`h-6 flex-1 rounded text-[12px] ${
                  themePref === opt.value
                    ? "bg-raised text-ink"
                    : "text-ink-4 hover:text-ink-2"
                }`}
              >
                {opt.label}
              </button>
            ))}
          </div>
          <div className="mt-3 text-[11px] font-medium tracking-wide text-ink-4 uppercase">
            Accent
          </div>
          <div className="mt-1.5 flex gap-2">
            {(Object.keys(ACCENTS) as AccentName[]).map((name) => (
              <button
                key={name}
                onClick={() => onAccent(name)}
                title={name}
                aria-label={`${name} accent`}
                className={`h-5 w-5 rounded-full border-2 ${
                  accent === name ? "border-ink" : "border-transparent"
                }`}
                style={{ background: accentSwatch(name) }}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
