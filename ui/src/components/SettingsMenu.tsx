import { useEffect, useRef, useState } from "react";
import {
  ACCENTS,
  type AccentName,
  type ThemePref,
  accentSwatch,
} from "../lib/theme";

const THEME_OPTIONS: { value: ThemePref; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

interface SettingsMenuProps {
  themePref: ThemePref;
  accent: AccentName;
  onThemePref: (pref: ThemePref) => void;
  onAccent: (accent: AccentName) => void;
}

export function SettingsMenu({
  themePref,
  accent,
  onThemePref,
  onAccent,
}: SettingsMenuProps) {
  const [open, setOpen] = useState(false);
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
        <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor">
          <path d="M7 0a7 7 0 0 0 0 14c.83 0 1.4-.6 1.4-1.3 0-.35-.14-.66-.36-.9a1.27 1.27 0 0 1 .95-2.13h1.57A3.43 3.43 0 0 0 14 6.24C13.88 2.72 10.76 0 7 0Zm-4.55 7a1.05 1.05 0 1 1 0-2.1 1.05 1.05 0 0 1 0 2.1Zm2.8-3.5a1.05 1.05 0 1 1 0-2.1 1.05 1.05 0 0 1 0 2.1Zm3.5 0a1.05 1.05 0 1 1 0-2.1 1.05 1.05 0 0 1 0 2.1Zm2.8 3.5a1.05 1.05 0 1 1 0-2.1 1.05 1.05 0 0 1 0 2.1Z" />
        </svg>
      </button>
      {open && (
        <div className="absolute top-9 right-0 z-50 w-52 rounded-md border border-edge-strong bg-panel p-3 shadow-xl">
          <div className="text-[11px] font-medium tracking-wide text-ink-4 uppercase">
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
