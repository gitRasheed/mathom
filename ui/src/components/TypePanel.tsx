// Detail panel: extension breakdown + largest files, scoped to the treemap
// view root. Self-fetching with the same live-scan throttle discipline as
// the treemap; colors index the shared category palette.

import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Row, type Snapshot, type TypePanelData } from "../lib/api";
import { isStale, reportUnlessStale } from "../lib/errors";
import { formatBytes, formatNumber, formatPercent } from "../lib/format";
import { PALETTE } from "../lib/palette";

const SCAN_REFRESH_MS = 700;

export interface TypePanelProps {
  snapshot: Snapshot | null;
  generation: number;
  /** Treemap view root — the panel describes this subtree. */
  rootId: number;
  /** Bumped by App after a delete so the numbers refresh. */
  revision: number;
  hideSystem: boolean;
  onSelectFile: (row: Row) => void;
}

export function TypePanel({
  snapshot,
  generation,
  rootId,
  revision,
  hideSystem,
  onSelectFile,
}: TypePanelProps) {
  const [data, setData] = useState<TypePanelData | null>(null);
  const seqRef = useRef(0);
  const lastFetchRef = useRef(0);

  const fetchStats = useCallback(() => {
    if (generation === 0) return;
    const seq = ++seqRef.current;
    lastFetchRef.current = performance.now();
    api
      .getTypeStats(generation, rootId, hideSystem)
      .then((d) => {
        if (seq === seqRef.current) setData(d);
      })
      .catch((e) => {
        // "unknown node" is expected while the tree is still empty at scan
        // start; the tick effect below retries naturally.
        if (!isStale(e) && !String(e).includes("unknown node")) {
          reportUnlessStale("loading file types", e);
        }
      });
  }, [generation, rootId, hideSystem]);

  // New scan: drop the previous scan's numbers immediately.
  useEffect(() => {
    setData(null);
  }, [generation]);

  // Root / filter / delete changed: refetch (previous numbers stay visible
  // until the new ones land — no flicker on drill).
  useEffect(() => {
    fetchStats();
  }, [fetchStats, revision]);

  // Live scan: throttled refresh on ticks, one final fetch on completion.
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
    <div className="flex w-64 shrink-0 flex-col border-l border-zinc-800">
      <div className="flex h-8 shrink-0 items-center border-b border-zinc-800 px-3">
        <span className="text-[11px] font-medium tracking-wide text-zinc-500 uppercase">
          File types
        </span>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto py-1.5">
        {data === null ? (
          // Loading (or no scan yet): stay quiet — flashing "no files"
          // before the first result lands reads as a wrong answer.
          generation === 0 ? (
            <div className="px-3 py-2 text-xs text-zinc-600">
              Appears during a scan
            </div>
          ) : null
        ) : data.totalFiles === 0 ? (
          <div className="px-3 py-2 text-xs text-zinc-600">No files here</div>
        ) : (
          <>
            {data.types.map((t) => (
              <TypeRow
                key={t.ext}
                color={PALETTE[t.category] ?? PALETTE[10]}
                label={t.ext === "" ? "(no extension)" : `.${t.ext}`}
                files={t.files}
                bytes={t.bytes}
                pct={data.totalBytes > 0 ? t.bytes / data.totalBytes : 0}
              />
            ))}
            {data.otherFiles > 0 && (
              <TypeRow
                color="transparent"
                label="other"
                files={data.otherFiles}
                bytes={data.otherBytes}
                pct={
                  data.totalBytes > 0 ? data.otherBytes / data.totalBytes : 0
                }
              />
            )}
            {data.topFiles.length > 0 && (
              <>
                <div className="mt-3 border-b border-zinc-800 px-3 pb-1.5 text-[11px] font-medium tracking-wide text-zinc-500 uppercase">
                  Largest files
                </div>
                <div className="pt-1">
                  {data.topFiles.map((f) => (
                    <button
                      key={f.id}
                      className="flex w-full items-center gap-2 px-3 py-1 text-left text-xs hover:bg-zinc-900"
                      onClick={() => onSelectFile(f)}
                      title={f.name}
                    >
                      <span className="min-w-0 flex-1 truncate text-zinc-300">
                        {f.name}
                      </span>
                      <span className="tnum shrink-0 text-zinc-500">
                        {formatBytes(f.size)}
                      </span>
                    </button>
                  ))}
                </div>
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
}

function TypeRow({
  color,
  label,
  files,
  bytes,
  pct,
}: {
  color: string;
  label: string;
  files: number;
  bytes: number;
  pct: number;
}) {
  return (
    <div className="flex items-center gap-2 px-3 py-1 text-xs">
      <span
        className="h-2 w-2 shrink-0 rounded-[3px]"
        style={{ background: color }}
      />
      <span className="min-w-0 flex-1 truncate text-zinc-300">{label}</span>
      <span
        className="tnum shrink-0 text-[11px] text-zinc-600"
        title={`${formatNumber(files)} files`}
      >
        {formatNumber(files)}
      </span>
      <span className="tnum w-16 shrink-0 text-right text-zinc-400">
        {formatBytes(bytes)}
      </span>
      <span className="tnum w-9 shrink-0 text-right text-[11px] text-zinc-600">
        {formatPercent(pct)}
      </span>
    </div>
  );
}
