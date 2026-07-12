import type { SearchHit } from "../lib/api";
import type { AccentName, ThemePref } from "../lib/theme";
import { ExportMenu } from "./ExportMenu";
import { ScanMenu } from "./ScanMenu";
import { SearchBox } from "./SearchBox";
import { SettingsMenu } from "./SettingsMenu";
import { WindowControls } from "./WindowControls";
import { PanelRightIcon } from "./icons";

interface ToolbarProps {
  scanning: boolean;
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
  return (
    // The toolbar IS the title bar (undecorated window): empty space drags
    // the window and double-click maximizes; children stay interactive.
    <header
      data-tauri-drag-region
      className="flex items-center gap-2 border-b border-edge px-3 py-2"
    >
      {scanning ? (
        <button
          onClick={onCancel}
          className="h-8 rounded-md border border-danger-edge/70 px-3.5 text-[13px] text-danger-ink hover:bg-danger-soft/40"
        >
          Cancel
        </button>
      ) : (
        <ScanMenu onScan={onScan} />
      )}
      {startError && (
        <span className="text-xs text-danger-ink">{startError}</span>
      )}
      <div className="ml-auto shrink-0">
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
        aria-label="File-types panel"
        className={`ml-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-md border ${
          typePanelOpen
            ? "border-edge-strong bg-raised text-ink"
            : "border-edge bg-panel text-ink-4 hover:bg-raised hover:text-ink-2"
        }`}
      >
        <PanelRightIcon />
      </button>
      <ExportMenu
        generation={generation}
        viewRootId={viewRootId}
        hideSystem={hideSystem}
        disabled={scanning || generation === 0}
      />
      <SettingsMenu
        hideSystem={hideSystem}
        themePref={themePref}
        accent={accent}
        onToggleHideSystem={onToggleHideSystem}
        onThemePref={onThemePref}
        onAccent={onAccent}
      />
      <WindowControls />
    </header>
  );
}
