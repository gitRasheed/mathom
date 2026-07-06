import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { Snapshot } from "../lib/api";
import { formatBytes, formatElapsed, formatNumber } from "../lib/format";

interface ToolbarProps {
  scanning: boolean;
  snapshot: Snapshot | null;
  startError: string | null;
  onScan: (path: string) => void;
  onCancel: () => void;
}

export function Toolbar({
  scanning,
  snapshot,
  startError,
  onScan,
  onCancel,
}: ToolbarProps) {
  const [path, setPath] = useState("");

  const browse = async () => {
    const picked = await open({
      directory: true,
      title: "Choose a folder to scan",
    });
    if (typeof picked === "string") setPath(picked);
  };

  const canScan = path.trim().length > 0 && !scanning;
  const submit = () => {
    if (canScan) onScan(path.trim());
  };

  const rate =
    snapshot && snapshot.elapsedMs > 0
      ? (snapshot.files + snapshot.dirs) / (snapshot.elapsedMs / 1000)
      : 0;

  return (
    <header className="flex items-center gap-2 border-b border-zinc-800 px-3 py-2">
      <span className="pr-2 text-sm font-semibold tracking-tight text-zinc-100">
        mathom
      </span>
      <input
        value={path}
        onChange={(e) => setPath(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") submit();
        }}
        placeholder="Folder to scan, e.g. C:\Users"
        spellCheck={false}
        className="h-8 w-[380px] rounded-md border border-zinc-800 bg-zinc-900 px-2.5 text-[13px] text-zinc-200 outline-none placeholder:text-zinc-600 focus:border-teal-700"
      />
      <button
        onClick={() => void browse()}
        className="h-8 rounded-md border border-zinc-800 bg-zinc-900 px-3 text-[13px] text-zinc-300 hover:bg-zinc-800"
      >
        Browse…
      </button>
      {scanning ? (
        <button
          onClick={onCancel}
          className="h-8 rounded-md border border-red-900/70 px-3.5 text-[13px] text-red-300 hover:bg-red-950/40"
        >
          Cancel
        </button>
      ) : (
        <button
          onClick={submit}
          disabled={!canScan}
          className="h-8 rounded-md bg-teal-600 px-4 text-[13px] font-medium text-white hover:bg-teal-500 disabled:cursor-not-allowed disabled:opacity-40"
        >
          Scan
        </button>
      )}
      {startError && <span className="text-xs text-red-400">{startError}</span>}
      <div className="tnum ml-auto flex items-center gap-3 text-xs text-zinc-500">
        {snapshot && snapshot.state !== "idle" && (
          <>
            <span>{formatNumber(snapshot.files)} files</span>
            <span>{formatNumber(snapshot.dirs)} folders</span>
            <span>{formatBytes(snapshot.bytes)}</span>
            <span>{formatElapsed(snapshot.elapsedMs)}</span>
            {scanning && (
              <span className="text-teal-500">
                {formatNumber(Math.round(rate))}/s
              </span>
            )}
          </>
        )}
      </div>
    </header>
  );
}
