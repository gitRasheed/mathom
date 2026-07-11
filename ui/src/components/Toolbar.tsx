import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { SearchHit, Snapshot } from "../lib/api";
import { formatBytes, formatElapsed, formatNumber } from "../lib/format";
import type { AccentName, ThemePref } from "../lib/theme";
import { ExportMenu } from "./ExportMenu";
import { SearchBox } from "./SearchBox";
import { SettingsMenu } from "./SettingsMenu";

interface ToolbarProps {
  scanning: boolean;
  snapshot: Snapshot | null;
  generation: number;
  viewRootId: number;
  startError: string | null;
  hideSystem: boolean;
  filter: string | null;
  typePanelOpen: boolean;
  themePref: ThemePref;
  accent: AccentName;
  onScan: (path: string) => void;
  onCancel: () => void;
  onToggleHideSystem: () => void;
  onToggleTypePanel: () => void;
  onSearchSelect: (hit: SearchHit) => void;
  onApplyFilter: (query: string | null) => void;
  onThemePref: (pref: ThemePref) => void;
  onAccent: (accent: AccentName) => void;
}

export function Toolbar({
  scanning,
  snapshot,
  generation,
  viewRootId,
  startError,
  hideSystem,
  filter,
  typePanelOpen,
  themePref,
  accent,
  onScan,
  onCancel,
  onToggleHideSystem,
  onToggleTypePanel,
  onSearchSelect,
  onApplyFilter,
  onThemePref,
  onAccent,
}: ToolbarProps) {
  const [path, setPath] = useState("");

  const browse = async () => {
    const picked = await open({
      directory: true,
      title: "Choose a folder to scan",
    });
    if (typeof picked === "string") {
      setPath(picked);
      // Picking from the dialog is the intent to scan it; no second click.
      if (!scanning) onScan(picked);
    }
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
    <header className="flex items-center gap-2 border-b border-edge px-3 py-2">
      <input
        value={path}
        onChange={(e) => setPath(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") submit();
        }}
        placeholder="Folder to scan, e.g. C:\Users"
        spellCheck={false}
        className="h-8 w-[380px] rounded-md border border-edge bg-panel px-2.5 text-[13px] text-ink outline-none placeholder:text-ink-5 focus:border-accent-edge"
      />
      <button
        onClick={() => void browse()}
        className="h-8 rounded-md border border-edge bg-panel px-3 text-[13px] text-ink-2 hover:bg-raised"
      >
        Browse…
      </button>
      {scanning ? (
        <button
          onClick={onCancel}
          className="h-8 rounded-md border border-danger-edge/70 px-3.5 text-[13px] text-danger-ink hover:bg-danger-soft/40"
        >
          Cancel
        </button>
      ) : (
        <button
          onClick={submit}
          disabled={!canScan}
          className="h-8 rounded-md bg-accent px-4 text-[13px] font-medium text-white hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-40"
        >
          Scan
        </button>
      )}
      {startError && (
        <span className="text-xs text-danger-ink">{startError}</span>
      )}
      <label
        className="ml-2 flex cursor-pointer items-center gap-1.5 text-[12px] text-ink-3 select-none"
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
      <div className="tnum ml-auto flex items-center gap-3 text-xs text-ink-4">
        {snapshot && snapshot.state !== "idle" && (
          <>
            <span>{formatNumber(snapshot.files)} files</span>
            <span>{formatNumber(snapshot.dirs)} folders</span>
            <span>{formatBytes(snapshot.bytes)}</span>
            <span>{formatElapsed(snapshot.elapsedMs)}</span>
            {scanning && (
              <span className="text-accent-ink">
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
          activeFilter={filter}
          canFilter={!scanning && generation > 0}
          onSelect={onSearchSelect}
          onApplyFilter={onApplyFilter}
        />
      </div>
      <button
        onClick={onToggleTypePanel}
        title="Show or hide the file-types panel"
        className={`ml-1 h-8 shrink-0 rounded-md border px-3 text-[12px] ${
          typePanelOpen
            ? "border-edge-strong bg-raised text-ink"
            : "border-edge bg-panel text-ink-4 hover:bg-raised hover:text-ink-2"
        }`}
      >
        File types
      </button>
      <ExportMenu
        generation={generation}
        viewRootId={viewRootId}
        hideSystem={hideSystem}
        disabled={scanning || generation === 0}
      />
      <SettingsMenu
        themePref={themePref}
        accent={accent}
        onThemePref={onThemePref}
        onAccent={onAccent}
      />
    </header>
  );
}
