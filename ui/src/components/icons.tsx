// Every inline SVG glyph lives here, so sizes, stroke weights, and
// viewBoxes stay consistent. All draw in currentColor.

import type { CSSProperties } from "react";

interface IconProps {
  className?: string;
  style?: CSSProperties;
}

export function ChevronIcon({ className, style }: IconProps) {
  return (
    <svg
      width="8"
      height="8"
      viewBox="0 0 8 8"
      className={className}
      style={style}
    >
      <path d="M2 0 L7 4 L2 8 Z" fill="currentColor" />
    </svg>
  );
}

export function PanelRightIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 14 14"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
    >
      <rect x="1" y="2" width="12" height="10" rx="1.5" />
      <line x1="9" y1="2" x2="9" y2="12" />
    </svg>
  );
}

export function ExportIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 14 14"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M7 8.75V1.75M4 4.5l3-3 3 3" />
      <path d="M1.75 10.75v1.5a1 1 0 0 0 1 1h8.5a1 1 0 0 0 1-1v-1.5" />
    </svg>
  );
}

export function PaletteIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor">
      <path d="M7 0a7 7 0 0 0 0 14c.83 0 1.4-.6 1.4-1.3 0-.35-.14-.66-.36-.9a1.27 1.27 0 0 1 .95-2.13h1.57A3.43 3.43 0 0 0 14 6.24C13.88 2.72 10.76 0 7 0Zm-4.55 7a1.05 1.05 0 1 1 0-2.1 1.05 1.05 0 0 1 0 2.1Zm2.8-3.5a1.05 1.05 0 1 1 0-2.1 1.05 1.05 0 0 1 0 2.1Zm3.5 0a1.05 1.05 0 1 1 0-2.1 1.05 1.05 0 0 1 0 2.1Zm2.8 3.5a1.05 1.05 0 1 1 0-2.1 1.05 1.05 0 0 1 0 2.1Z" />
    </svg>
  );
}

export function MinimizeIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" stroke="currentColor">
      <line x1="0" y1="5" x2="10" y2="5" />
    </svg>
  );
}

export function MaximizeIcon() {
  return (
    <svg
      width="10"
      height="10"
      viewBox="0 0 10 10"
      fill="none"
      stroke="currentColor"
    >
      <rect x="0.5" y="0.5" width="9" height="9" />
    </svg>
  );
}

export function RestoreIcon() {
  return (
    <svg
      width="10"
      height="10"
      viewBox="0 0 10 10"
      fill="none"
      stroke="currentColor"
    >
      <rect x="0.5" y="2.5" width="7" height="7" />
      <path d="M2.5 2.5 v-2 h7 v7 h-2" />
    </svg>
  );
}

export function CloseIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" stroke="currentColor">
      <path d="M0 0 L10 10 M10 0 L0 10" />
    </svg>
  );
}
