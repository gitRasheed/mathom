import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { SearchHit, Snapshot } from "../lib/api";
import { formatBytes, formatElapsed, formatNumber } from "../lib/format";
import { SearchBox } from "./SearchBox";

interface ToolbarProps {
  scanning: boolean;
  snapshot: Snapshot | null;
  generation: number;
  startError: string | null;
  hideSystem: boolean;
  typePanelOpen: boolean;
  onScan: (path: string) => void;
  onCancel: () => void;
  onToggleHideSystem: () => void;
  onToggleTypePanel: () => void;
  onSearchSelect: (hit: SearchHit) => void;
}

export function Toolbar({
  scanning,
  snapshot,
  generation,
  startError,
  hideSystem,
  typePanelOpen,
  onScan,
  onCancel,
  onToggleHideSystem,
  onToggleTypePanel,
  onSearchSelect,
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
      <label
        className="ml-2 flex cursor-pointer items-center gap-1.5 text-[12px] text-zinc-400 select-none"
        title="Hide OS/system files (pagefile, hiberfil, System Volume Information, …)"
      >
        <input
          type="checkbox"
          className="accent-teal-600"
          checked={hideSystem}
          onChange={onToggleHideSystem}
        />
        Hide system files
      </label>
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
      <div className="ml-3 shrink-0">
        <SearchBox
          generation={generation}
          hideSystem={hideSystem}
          onSelect={onSearchSelect}
        />
      </div>
      <button
        onClick={onToggleTypePanel}
        title="Show or hide the file-types panel"
        className={`ml-1 h-8 shrink-0 rounded-md border px-3 text-[12px] ${
          typePanelOpen
            ? "border-zinc-700 bg-zinc-800 text-zinc-200"
            : "border-zinc-800 bg-zinc-900 text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300"
        }`}
      >
        File types
      </button>
    </header>
  );
}
