// Canvas treemap. Rendering strategy (see ARCHITECTURE.md):
// - Rust ships a flat rect list (parents before children = painter's order).
// - The full map is baked once per layout into an offscreen canvas — fills
//   batched by category color, edges snapped to device pixels (1-device-px
//   gaps instead of strokes), one stretched highlight sprite per leaf as a
//   cheap cushion approximation — then blitted. Hover/selection outlines
//   live on a separate overlay canvas so mousemove never rebakes.
// - Drill-down animates the cached bitmap (drawImage source-rect zoom),
//   then swaps in the freshly baked layout.

import {
  Fragment,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { api, type Crumb, type Row, type Snapshot, type TreemapRect } from "../lib/api";
import { formatBytes, formatPercent } from "../lib/format";

const PALETTE: readonly string[] = [
  "#30323a", // 0 directory plate
  "#a855f7", // 1 video
  "#22c55e", // 2 audio
  "#eab308", // 3 image
  "#f97316", // 4 archive
  "#3b82f6", // 5 document
  "#06b6d4", // 6 code
  "#ec4899", // 7 executable
  "#64748b", // 8 system
  "#84cc16", // 9 data
  "#71717a", // 10 other
];

const SCAN_REFRESH_MS = 400;
const ZOOM_MS = 220;

let highlightSprite: HTMLCanvasElement | null = null;

function getHighlightSprite(): HTMLCanvasElement {
  if (highlightSprite) return highlightSprite;
  const c = document.createElement("canvas");
  c.width = 128;
  c.height = 128;
  const ctx = c.getContext("2d")!;
  const g = ctx.createRadialGradient(40, 32, 0, 64, 64, 120);
  g.addColorStop(0, "rgba(255,255,255,0.30)");
  g.addColorStop(0.55, "rgba(255,255,255,0.03)");
  g.addColorStop(1, "rgba(0,0,0,0.28)");
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, 128, 128);
  highlightSprite = c;
  return c;
}

interface Snapped {
  x: number;
  y: number;
  w: number;
  h: number;
}

function snap(r: TreemapRect, dpr: number, gap: number): Snapped {
  const x0 = Math.round(r.x * dpr);
  const y0 = Math.round(r.y * dpr);
  const x1 = Math.round((r.x + r.w) * dpr);
  const y1 = Math.round((r.y + r.h) * dpr);
  return { x: x0, y: y0, w: x1 - x0 - gap, h: y1 - y0 - gap };
}

interface TooltipData {
  name: string;
  size: string;
  pct: string;
  path: string;
}

export interface TreemapProps {
  snapshot: Snapshot | null;
  generation: number;
  selected: number | null;
  onSelect: (id: number) => void;
}

export function Treemap({ snapshot, generation, selected, onSelect }: TreemapProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const baseRef = useRef<HTMLCanvasElement>(null);
  const overlayRef = useRef<HTMLCanvasElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);

  const rectsRef = useRef<TreemapRect[]>([]);
  const byIdRef = useRef<Map<number, TreemapRect>>(new Map());
  const offscreenRef = useRef<HTMLCanvasElement | null>(null);
  const rootIdRef = useRef(0);
  const lastFetchRef = useRef(0);
  const fetchSeqRef = useRef(0);
  const zoomRafRef = useRef(0);
  const hoverRef = useRef<number | null>(null);
  const tooltipSeqRef = useRef(0);

  const [crumbs, setCrumbs] = useState<Crumb[]>([]);
  const [tooltip, setTooltip] = useState<TooltipData | null>(null);
  const [hasRects, setHasRects] = useState(false);

  const drawOverlay = useCallback(() => {
    const overlay = overlayRef.current;
    if (!overlay) return;
    const ctx = overlay.getContext("2d")!;
    ctx.clearRect(0, 0, overlay.width, overlay.height);
    const dpr = window.devicePixelRatio || 1;
    const outline = (id: number | null, color: string, width: number) => {
      if (id === null) return;
      const r = byIdRef.current.get(id);
      if (!r) return;
      const s = snap(r, dpr, 0);
      ctx.strokeStyle = color;
      ctx.lineWidth = width;
      ctx.strokeRect(s.x + width / 2, s.y + width / 2, s.w - width, s.h - width);
    };
    outline(selected, "#f4f4f5", 2);
    if (hoverRef.current !== selected) outline(hoverRef.current, "#2dd4bf", 2);
  }, [selected]);

  const blit = useCallback(() => {
    const base = baseRef.current;
    const off = offscreenRef.current;
    if (!base || !off) return;
    const ctx = base.getContext("2d")!;
    ctx.clearRect(0, 0, base.width, base.height);
    ctx.drawImage(off, 0, 0);
    drawOverlay();
  }, [drawOverlay]);

  const bake = useCallback(() => {
    const base = baseRef.current;
    if (!base || base.width === 0) return;
    let off = offscreenRef.current;
    if (!off) {
      off = document.createElement("canvas");
      offscreenRef.current = off;
    }
    off.width = base.width;
    off.height = base.height;
    const ctx = off.getContext("2d")!;
    const dpr = window.devicePixelRatio || 1;
    const rects = rectsRef.current;

    ctx.fillStyle = "#0f1115";
    ctx.fillRect(0, 0, off.width, off.height);

    // Pass 1: directory plates (all one color, order-independent).
    ctx.fillStyle = PALETTE[0];
    ctx.beginPath();
    for (const r of rects) {
      if (!r.isDir) continue;
      const s = snap(r, dpr, 0);
      if (s.w > 0 && s.h > 0) ctx.rect(s.x, s.y, s.w, s.h);
    }
    ctx.fill();

    // Pass 2: leaves batched by category (leaves never overlap each other).
    const buckets: TreemapRect[][] = PALETTE.map(() => []);
    for (const r of rects) {
      if (!r.isDir) buckets[r.category]?.push(r);
    }
    for (let c = 0; c < buckets.length; c++) {
      const bucket = buckets[c];
      if (bucket.length === 0) continue;
      ctx.fillStyle = PALETTE[c];
      ctx.beginPath();
      for (const r of bucket) {
        const s = snap(r, dpr, 1);
        if (s.w > 0 && s.h > 0) ctx.rect(s.x, s.y, s.w, s.h);
      }
      ctx.fill();
    }

    // Pass 3: cushion-ish highlight, one stretched sprite per visible leaf.
    const sprite = getHighlightSprite();
    for (const r of rects) {
      if (r.isDir) continue;
      const s = snap(r, dpr, 1);
      if (s.w > 3 && s.h > 3) ctx.drawImage(sprite, s.x, s.y, s.w, s.h);
    }

    if (zoomRafRef.current === 0) blit();
  }, [blit]);

  const fetchLayout = useCallback(async () => {
    const container = containerRef.current;
    if (!container || generation === 0) return;
    const w = container.clientWidth;
    const h = container.clientHeight;
    if (w < 10 || h < 10) return;

    const seq = ++fetchSeqRef.current;
    const rootId = rootIdRef.current;
    lastFetchRef.current = performance.now();
    try {
      const rects = await api.getTreemap(generation, rootId, w, h);
      if (seq !== fetchSeqRef.current || rootId !== rootIdRef.current) return;
      rectsRef.current = rects;
      byIdRef.current = new Map(rects.map((r) => [r.id, r]));
      setHasRects(rects.length > 0);
      bake();
    } catch {
      // stale generation
    }
  }, [generation, bake]);

  const refreshCrumbs = useCallback(() => {
    if (generation === 0) return;
    api
      .getAncestors(generation, rootIdRef.current)
      .then(setCrumbs)
      .catch(() => {});
  }, [generation]);

  // New scan: reset drill state and clear the canvas.
  useEffect(() => {
    rootIdRef.current = 0;
    rectsRef.current = [];
    byIdRef.current = new Map();
    setHasRects(false);
    setCrumbs([]);
    setTooltip(null);
    hoverRef.current = null;
    offscreenRef.current = null;
    const base = baseRef.current;
    if (base) base.getContext("2d")!.clearRect(0, 0, base.width, base.height);
    if (generation !== 0) {
      void fetchLayout();
      refreshCrumbs();
    }
  }, [generation, fetchLayout, refreshCrumbs]);

  // Live scan: refetch layout on ticks, throttled; always refetch on the
  // final (done/cancelled) snapshot.
  const prevStateRef = useRef<string | undefined>(undefined);
  useEffect(() => {
    if (!snapshot || snapshot.generation !== generation) return;
    const prev = prevStateRef.current;
    prevStateRef.current = snapshot.state;
    if (snapshot.state === "scanning") {
      if (performance.now() - lastFetchRef.current >= SCAN_REFRESH_MS) {
        void fetchLayout();
        refreshCrumbs();
      }
    } else if (prev === "scanning") {
      void fetchLayout();
      refreshCrumbs();
    }
  }, [snapshot, generation, fetchLayout, refreshCrumbs]);

  // Resize: match canvas backing stores to the container at device pixels.
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    let timer = 0;
    const applySize = () => {
      const dpr = window.devicePixelRatio || 1;
      const w = Math.round(container.clientWidth * dpr);
      const h = Math.round(container.clientHeight * dpr);
      for (const c of [baseRef.current, overlayRef.current]) {
        if (c && (c.width !== w || c.height !== h)) {
          c.width = w;
          c.height = h;
        }
      }
      void fetchLayout();
    };
    applySize();
    const ro = new ResizeObserver(() => {
      window.clearTimeout(timer);
      timer = window.setTimeout(applySize, 150);
    });
    ro.observe(container);
    return () => {
      window.clearTimeout(timer);
      ro.disconnect();
    };
  }, [fetchLayout]);

  useEffect(() => {
    drawOverlay();
  }, [selected, drawOverlay]);

  const hitTest = useCallback((cssX: number, cssY: number): TreemapRect | null => {
    const rects = rectsRef.current;
    // Reverse iteration = deepest-first (parents are emitted before children).
    for (let i = rects.length - 1; i >= 0; i--) {
      const r = rects[i];
      if (cssX >= r.x && cssX < r.x + r.w && cssY >= r.y && cssY < r.y + r.h) {
        return r;
      }
    }
    return null;
  }, []);

  const handleMove = useCallback(
    (e: React.MouseEvent) => {
      const container = containerRef.current;
      const tip = tooltipRef.current;
      if (!container) return;
      const bounds = container.getBoundingClientRect();
      const x = e.clientX - bounds.left;
      const y = e.clientY - bounds.top;
      if (tip) {
        const tx = Math.min(x + 14, bounds.width - 260);
        const ty = Math.min(y + 16, bounds.height - 70);
        tip.style.transform = `translate(${Math.max(0, tx)}px, ${Math.max(0, ty)}px)`;
      }

      const hit = hitTest(x, y);
      const id = hit?.id ?? null;
      if (id === hoverRef.current) return;
      hoverRef.current = id;
      drawOverlay();

      const seq = ++tooltipSeqRef.current;
      if (id === null || generation === 0) {
        setTooltip(null);
        return;
      }
      Promise.all([api.getNode(generation, id), api.getPath(generation, id)])
        .then(([node, path]: [Row | null, string]) => {
          if (seq !== tooltipSeqRef.current || !node) return;
          setTooltip({
            name: node.name,
            size: formatBytes(node.size),
            pct: formatPercent(node.pct),
            path,
          });
        })
        .catch(() => {});
    },
    [generation, hitTest, drawOverlay],
  );

  const handleLeave = useCallback(() => {
    hoverRef.current = null;
    tooltipSeqRef.current++;
    setTooltip(null);
    drawOverlay();
  }, [drawOverlay]);

  const drillTo = useCallback(
    (id: number, zoomFrom?: TreemapRect) => {
      if (id === rootIdRef.current) return;
      rootIdRef.current = id;
      setTooltip(null);
      hoverRef.current = null;

      // Bitmap zoom while the new layout is fetched (drill-down only —
      // breadcrumb zoom-out swaps instantly).
      const base = baseRef.current;
      const off = offscreenRef.current;
      if (zoomFrom && base && off) {
        const dpr = window.devicePixelRatio || 1;
        const target = snap(zoomFrom, dpr, 0);
        const frozen = document.createElement("canvas");
        frozen.width = off.width;
        frozen.height = off.height;
        frozen.getContext("2d")!.drawImage(off, 0, 0);
        const start = performance.now();
        const ctx = base.getContext("2d")!;
        const step = () => {
          const t = Math.min(1, (performance.now() - start) / ZOOM_MS);
          const ease = 1 - (1 - t) * (1 - t);
          const sx = target.x * ease;
          const sy = target.y * ease;
          const sw = frozen.width + (target.w - frozen.width) * ease;
          const sh = frozen.height + (target.h - frozen.height) * ease;
          ctx.clearRect(0, 0, base.width, base.height);
          ctx.drawImage(frozen, sx, sy, sw, sh, 0, 0, base.width, base.height);
          if (t < 1) {
            zoomRafRef.current = requestAnimationFrame(step);
          } else {
            zoomRafRef.current = 0;
            blit();
          }
        };
        cancelAnimationFrame(zoomRafRef.current);
        zoomRafRef.current = requestAnimationFrame(step);
      }

      void fetchLayout();
      refreshCrumbs();
    },
    [blit, fetchLayout, refreshCrumbs],
  );

  const handleClick = useCallback(
    (e: React.MouseEvent) => {
      const bounds = containerRef.current!.getBoundingClientRect();
      const hit = hitTest(e.clientX - bounds.left, e.clientY - bounds.top);
      if (hit) onSelect(hit.id);
    },
    [hitTest, onSelect],
  );

  const handleDoubleClick = useCallback(
    (e: React.MouseEvent) => {
      const bounds = containerRef.current!.getBoundingClientRect();
      const hit = hitTest(e.clientX - bounds.left, e.clientY - bounds.top);
      if (hit?.isDir) drillTo(hit.id, hit);
    },
    [hitTest, drillTo],
  );

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div className="flex h-8 shrink-0 items-center gap-1 overflow-hidden border-b border-zinc-800 px-3 text-xs">
        {crumbs.length === 0 ? (
          <span className="text-zinc-600">Treemap</span>
        ) : (
          crumbs.map((c, i) => (
            <Fragment key={c.id}>
              {i > 0 && <span className="shrink-0 text-zinc-600">›</span>}
              <button
                className={`max-w-56 truncate ${
                  i === crumbs.length - 1
                    ? "text-zinc-200"
                    : "text-zinc-500 hover:text-zinc-200"
                }`}
                onClick={() => drillTo(c.id)}
                title={c.name}
              >
                {c.name}
              </button>
            </Fragment>
          ))
        )}
      </div>
      <div
        ref={containerRef}
        className="relative min-h-0 flex-1 overflow-hidden"
        onMouseMove={handleMove}
        onMouseLeave={handleLeave}
        onClick={handleClick}
        onDoubleClick={handleDoubleClick}
      >
        <canvas ref={baseRef} className="absolute inset-0 h-full w-full" />
        <canvas ref={overlayRef} className="absolute inset-0 h-full w-full" />
        {!hasRects && (
          <div className="absolute inset-0 flex items-center justify-center text-xs text-zinc-600">
            {generation === 0 ? "Treemap appears here during a scan" : "Waiting for data…"}
          </div>
        )}
        {tooltip && (
          <div
            ref={tooltipRef}
            className="pointer-events-none absolute left-0 top-0 z-10 max-w-64 rounded-md border border-zinc-700 bg-zinc-900/95 px-2.5 py-1.5 text-xs shadow-lg"
          >
            <div className="truncate font-medium text-zinc-100">{tooltip.name}</div>
            <div className="tnum text-zinc-400">
              {tooltip.size} · {tooltip.pct} of parent
            </div>
            <div className="truncate text-[11px] text-zinc-500">{tooltip.path}</div>
          </div>
        )}
      </div>
    </div>
  );
}
