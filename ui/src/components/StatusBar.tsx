import type { Snapshot } from "../lib/api";
import { formatElapsed, formatNumber } from "../lib/format";

interface StatusBarProps {
  snapshot: Snapshot | null;
  selectedPath: string | null;
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

export function StatusBar({ snapshot, selectedPath }: StatusBarProps) {
  const state = snapshot?.state;
  return (
    <footer className="flex h-7 items-center gap-3 border-t border-zinc-800 px-3 text-xs text-zinc-500">
      <span
        className={`shrink-0 ${
          state === "failed"
            ? "text-red-400"
            : state === "scanning"
              ? "text-teal-400"
              : ""
        }`}
      >
        {stateLabel(snapshot)}
      </span>
      {snapshot !== null && snapshot.errors > 0 && (
        <span className="tnum shrink-0 text-amber-500/90">
          {formatNumber(snapshot.errors)} unreadable
        </span>
      )}
      <span
        className="min-w-0 flex-1 truncate text-center text-zinc-600"
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
