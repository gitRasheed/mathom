// Extension breakdown + largest files for the treemap view root.

import { useCallback, useEffect, useRef, useState } from "react";
import { List, type RowComponentProps } from "react-window";
import {
  api,
  type Row,
  type Snapshot,
  type TypePanelData,
  type TypeStat,
} from "../lib/api";
import { isStale, reportUnlessStale } from "../lib/errors";
import { formatBytes, formatNumber, formatPercent } from "../lib/format";
import { PALETTE } from "../lib/palette";

const SCAN_REFRESH_MS = 700;
const TYPE_ROW_HEIGHT = 24;

export interface TypePanelProps {
  snapshot: Snapshot | null;
  generation: number;
  /** Treemap view root — the panel describes this subtree. */
  rootId: number;
  /** Bumped by App after a delete so the numbers refresh. */
  revision: number;
  hideSystem: boolean;
  /** Active view filter (search grammar) or null. */
  filter: string | null;
  onSelectFile: (row: Row) => void;
  /** Extensions in the active `ext:` filter — their rows get a tint. */
  activeExts: string[];
  /** Click = filter to this type; shift-click (additive) = add/remove it. */
  onFilterType: (ext: string, additive: boolean) => void;
}

export function TypePanel({
  snapshot,
  generation,
  rootId,
  revision,
  hideSystem,
  filter,
  onSelectFile,
  activeExts,
  onFilterType,
}: TypePanelProps) {
  const [data, setData] = useState<TypePanelData | null>(null);
  const seqRef = useRef(0);
  const lastFetchRef = useRef(0);

  const fetchStats = useCallback(() => {
    if (generation === 0) return;
    const seq = ++seqRef.current;
    lastFetchRef.current = performance.now();
    const full = api.getTypeStats(generation, rootId, hideSystem, filter);
    // Facet self-exclusion: the type rows ignore a pure type filter; totals/largest stay filtered.
    const picker =
      activeExts.length > 0
        ? api.getTypeStats(generation, rootId, hideSystem, null)
        : full;
    Promise.all([full, picker])
      .then(([f, p]) => {
        if (seq === seqRef.current) setData({ ...f, types: p.types });
      })
      .catch((e) => {
        // The tree can still be empty at scan start; ticks retry naturally.
        if (!isStale(e) && !String(e).includes("unknown node")) {
          reportUnlessStale("loading file types", e);
        }
      });
  }, [generation, rootId, hideSystem, filter, activeExts]);

  useEffect(() => {
    setData(null);
  }, [generation]);

  useEffect(() => {
    fetchStats();
  }, [fetchStats, revision]);

  const prevStateRef = useRef<string | undefined>(undefined);
  useEffect(() => {
    if (!snapshot || snapshot.generation !== generation) return;
    const prev = prevStateRef.current;
    prevStateRef.current = snapshot.state;
    if (snapshot.state === "scanning") {
      if (performance.now() - lastFetchRef.current >= SCAN_REFRESH_MS) {
        fetchStats();
      }
    } else if (prev === "scanning") {
      fetchStats();
    }
  }, [snapshot, generation, fetchStats]);

  return (
    <div className="flex w-64 shrink-0 flex-col border-l border-edge">
      <div className="flex h-8 shrink-0 items-center border-b border-edge px-3">
        <span className="text-[11px] font-medium tracking-wide text-ink-4 uppercase">
          File types
        </span>
      </div>
      <div className="min-h-0 flex-1 pt-1.5">
        {data === null ? (
          generation === 0 ? (
            <div className="px-3 py-2 text-xs text-ink-5">
              Appears during a scan
            </div>
          ) : null
        ) : data.totalFiles === 0 ? (
          <div className="px-3 py-2 text-xs text-ink-5">No files here</div>
        ) : (
          <List
            rowComponent={TypeRow}
            rowCount={data.types.length}
            rowHeight={TYPE_ROW_HEIGHT}
            rowProps={{
              types: data.types,
              totalBytes: data.totalBytes,
              activeExts,
              onFilterType,
            }}
            className="h-full"
          />
        )}
      </div>
      {data !== null && data.topFiles.length > 0 && (
        <div className="shrink-0 border-t border-edge pb-1.5">
          <div className="px-3 pt-2 pb-1 text-[11px] font-medium tracking-wide text-ink-4 uppercase">
            Largest files
          </div>
          {data.topFiles.map((f) => (
            <button
              key={f.id}
              className="flex w-full items-center gap-2 px-3 py-1 text-left text-xs hover:bg-hush"
              onClick={() => onSelectFile(f)}
              title={f.name}
            >
              <span className="min-w-0 flex-1 truncate text-ink-2">
                {f.name}
              </span>
              <span className="tnum shrink-0 text-ink-4">
                {formatBytes(f.size)}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

interface TypeRowsProps {
  types: TypeStat[];
  totalBytes: number;
  activeExts: string[];
  onFilterType: (ext: string, additive: boolean) => void;
}

function TypeRow({
  index,
  style,
  types,
  totalBytes,
  activeExts,
  onFilterType,
}: RowComponentProps<TypeRowsProps>) {
  const t = types[index];
  // "(no extension)" can't be expressed in the search grammar — not clickable.
  const clickable = t.ext !== "";
  const active = activeExts.includes(t.ext);
  return (
    <div
      style={style}
      className={`flex items-center gap-2 px-3 text-xs ${
        active ? "bg-accent-soft/25" : clickable ? "hover:bg-hush" : ""
      }`}
      title={clickable ? "Click filters to this type · shift-click adds" : ""}
      onClick={(e) => {
        if (clickable) onFilterType(t.ext, e.shiftKey);
      }}
    >
      <span
        className="h-2 w-2 shrink-0 rounded-[3px]"
        style={{ background: PALETTE[t.category] ?? PALETTE[10] }}
      />
      <span className="min-w-0 flex-1 truncate text-ink-2">
        {t.ext === "" ? "(no extension)" : `.${t.ext}`}
      </span>
      <span
        className="tnum shrink-0 text-[11px] text-ink-5"
        title={`${formatNumber(t.files)} files`}
      >
        {formatNumber(t.files)}
      </span>
      <span className="tnum w-16 shrink-0 text-right text-ink-3">
        {formatBytes(t.bytes)}
      </span>
      <span className="tnum w-9 shrink-0 text-right text-[11px] text-ink-5">
        {formatPercent(totalBytes > 0 ? t.bytes / totalBytes : 0)}
      </span>
    </div>
  );
}
