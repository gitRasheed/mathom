import type { Snapshot } from "../lib/api";
import { formatElapsed, formatNumber } from "../lib/format";

interface StatusBarProps {
  snapshot: Snapshot | null;
  selectedPath: string | null;
  uiError: string | null;
}

function stateLabel(snapshot: Snapshot | null): string {
  switch (snapshot?.state) {
    case "scanning":
      return "Scanning…";
    case "done":
      return `Scan complete in ${formatElapsed(snapshot.elapsedMs)}`;
    case "cancelled":
      return "Scan cancelled";
    case "failed":
      return `Scan failed: ${snapshot.rootError ?? "unknown error"}`;
    default:
      return "Ready";
  }
}

export function StatusBar({ snapshot, selectedPath, uiError }: StatusBarProps) {
  const state = snapshot?.state;
  return (
    <footer className="flex h-7 items-center gap-3 border-t border-edge px-3 text-xs text-ink-4">
      <span
        className={`shrink-0 ${
          state === "failed"
            ? "text-danger-ink"
            : state === "scanning"
              ? "text-accent-ink"
              : ""
        }`}
      >
        {stateLabel(snapshot)}
      </span>
      {uiError && (
        <span
          className="max-w-96 shrink-0 truncate text-danger-ink"
          title={uiError}
        >
          {uiError}
        </span>
      )}
      {snapshot !== null && snapshot.errors > 0 && (
        <span className="tnum shrink-0 text-warn/90">
          {formatNumber(snapshot.errors)} unreadable
        </span>
      )}
      <span
        className="min-w-0 flex-1 truncate text-center text-ink-5"
        title={selectedPath ?? undefined}
      >
        {selectedPath ?? ""}
      </span>
      <span className="tnum shrink-0">
        {formatNumber(snapshot?.nodes ?? 0)} nodes
      </span>
    </footer>
  );
}
