// Canvas treemap. Rendering strategy (see ARCHITECTURE.md):
// - Rust ships a flat rect list (parents before children = painter's order).
// - The full map is baked once per layout into an offscreen canvas — fills
//   batched by category color, edges snapped to device pixels (1-device-px
//   gaps instead of strokes), one stretched highlight sprite per leaf as a
//   cheap cushion approximation — then blitted. Hover/selection outlines
//   live on a separate overlay canvas so mousemove never rebakes.
//
// Interaction model: the view root is CONTROLLED by App (`rootId`), which
// derives it from the selection. This component only reports intent:
// onSelect (tile clicked), onHover (tree-row sync), onNavigate (breadcrumb
// and zoom gestures).
//
// IMPORTANT invariant: every callback in the canvas pipeline (drawOverlay →
// blit → bake → fetchLayout → drillTo) is identity-stable — props they need
// are mirrored into refs each render. A previous version let drawOverlay
// depend on `hoveredId`, which cascaded into the reset/resize effects and
// made every hover wipe the canvas and refetch the full layout (IPC flood,
// webview crashes). Effects below must only depend on values whose change
// genuinely requires that effect.

import {
  Fragment,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import {
  api,
  type Crumb,
  type Row,
  type Snapshot,
  type TreemapRect,
} from "../lib/api";
import { isStale, reportUnlessStale } from "../lib/errors";
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
const TOOLTIP_DELAY_MS = 120;

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
  /** View root — controlled by App, derived from the selection. */
  rootId: number;
  /** Bumped by App on an out-of-band tree change (a delete) to force relayout. */
  revision: number;
  /** Hide OS/system entries; forwarded to the layout query. */
  hideSystem: boolean;
  selected: number | null;
  /** Node hovered in the tree pane — outlined here when visible. */
  hoveredId: number | null;
  onSelect: (rect: TreemapRect) => void;
  onHover: (id: number | null) => void;
  /** Request a different view root (breadcrumb, zoom in/out gestures). */
  onNavigate: (id: number) => void;
  /** Right-click on a tile: (id, viewport clientX/clientY). */
  onContext: (id: number, x: number, y: number) => void;
}

export function Treemap({
  snapshot,
  generation,
  rootId,
  revision,
  hideSystem,
  selected,
  hoveredId,
  onSelect,
  onHover,
  onNavigate,
  onContext,
}: TreemapProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const baseRef = useRef<HTMLCanvasElement>(null);
  const overlayRef = useRef<HTMLCanvasElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);

  const rectsRef = useRef<TreemapRect[]>([]);
  const byIdRef = useRef<Map<number, TreemapRect>>(new Map());
  const offscreenRef = useRef<HTMLCanvasElement | null>(null);
  const rootIdRef = useRef(0);
  // Between drill start and the new layout landing, the rect list describes
  // the OLD view — clicks/hover against it would hit stale geometry.
  const hitFrozenRef = useRef(false);
  // Root the current crumbs belong to; names are immutable, so crumbs only
  // need refetching when the view root changes (not on every tick).
  const crumbsRootRef = useRef<number | null>(null);
  const lastFetchRef = useRef(0);
  const fetchSeqRef = useRef(0);
  const zoomRafRef = useRef(0);
  const mouseOverRef = useRef<number | null>(null);
  const tooltipSeqRef = useRef(0);
  const tooltipTimerRef = useRef(0);
  const lastMouseRef = useRef({ x: 0, y: 0 });

  // Prop mirrors so the pipeline callbacks stay identity-stable.
  const generationRef = useRef(generation);
  generationRef.current = generation;
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  const hoveredIdRef = useRef(hoveredId);
  hoveredIdRef.current = hoveredId;
  const hideSystemRef = useRef(hideSystem);
  hideSystemRef.current = hideSystem;

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
      // The view root fills the canvas; outlining it is pure noise.
      if (id === null || id === rootIdRef.current) return;
      const r = byIdRef.current.get(id);
      if (!r) return;
      const s = snap(r, dpr, 0);
      ctx.strokeStyle = color;
      ctx.lineWidth = width;
      ctx.strokeRect(s.x + width / 2, s.y + width / 2, s.w - width, s.h - width);
    };
    outline(selectedRef.current, "#f4f4f5", 2);
    if (hoveredIdRef.current !== selectedRef.current) {
      outline(hoveredIdRef.current, "#2dd4bf", 2);
    }
  }, []);

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

  const refreshCrumbs = useCallback(() => {
    const generation = generationRef.current;
    if (generation === 0) return;
    const forRoot = rootIdRef.current;
    api
      .getAncestors(generation, forRoot)
      .then((crumbs) => {
        crumbsRootRef.current = forRoot;
        setCrumbs(crumbs);
      })
      .catch((e) => {
        // "unknown node" is expected while the tree is still empty at scan
        // start; fetchLayout retries crumbs on its next success.
        if (!isStale(e) && !String(e).includes("unknown node")) {
          reportUnlessStale("loading breadcrumbs", e);
        }
      });
  }, []);

  const fetchLayout = useCallback(async () => {
    const container = containerRef.current;
    const generation = generationRef.current;
    if (!container || generation === 0) {
      hitFrozenRef.current = false;
      return;
    }
    const w = container.clientWidth;
    const h = container.clientHeight;
    if (w < 10 || h < 10) {
      hitFrozenRef.current = false;
      return;
    }

    const seq = ++fetchSeqRef.current;
    const forRoot = rootIdRef.current;
    lastFetchRef.current = performance.now();
    try {
      const rects = await api.getTreemap(generation, forRoot, w, h, hideSystemRef.current);
      if (seq !== fetchSeqRef.current || forRoot !== rootIdRef.current) return;
      rectsRef.current = rects;
      byIdRef.current = new Map(rects.map((r) => [r.id, r]));
      hitFrozenRef.current = false;
      setHasRects(rects.length > 0);
      bake();
      if (crumbsRootRef.current !== forRoot) refreshCrumbs();
    } catch (e) {
      reportUnlessStale("loading treemap", e);
      if (seq === fetchSeqRef.current) hitFrozenRef.current = false;
    }
  }, [bake, refreshCrumbs]);

  const drillTo = useCallback(
    (id: number) => {
      if (id === rootIdRef.current) return;
      const zoomFrom = byIdRef.current.get(id);
      rootIdRef.current = id;
      hitFrozenRef.current = true;
      setTooltip(null);
      mouseOverRef.current = null;

      // Bitmap zoom toward the target's old rect while the layout loads;
      // navigating to something not in view (breadcrumb, tree) swaps flat.
      const base = baseRef.current;
      const off = offscreenRef.current;
      if (zoomFrom && zoomFrom.isDir && base && off) {
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
    },
    [blit, fetchLayout],
  );

  // The view root is controlled: App changed it (selection / navigation).
  useEffect(() => {
    drillTo(rootId);
  }, [rootId, drillTo]);

  // New scan: reset drill state and clear the canvas. `drillTo`/`fetchLayout`
  // are identity-stable, so this runs only on real generation changes.
  useEffect(() => {
    rootIdRef.current = 0;
    rectsRef.current = [];
    byIdRef.current = new Map();
    setHasRects(false);
    setCrumbs([]);
    setTooltip(null);
    mouseOverRef.current = null;
    hitFrozenRef.current = false;
    crumbsRootRef.current = null;
    offscreenRef.current = null;
    const base = baseRef.current;
    if (base) base.getContext("2d")!.clearRect(0, 0, base.width, base.height);
    if (generation !== 0) void fetchLayout();
  }, [generation, fetchLayout]);

  // Selection / cross-pane hover changed: redraw the two outlines, nothing else.
  useEffect(() => {
    drawOverlay();
  }, [selected, hoveredId, drawOverlay]);

  // A delete (or other out-of-band tree change) bumps `revision`; relayout so
  // the removed tile disappears. fetchLayout is identity-stable (see header),
  // so this only fires on a real revision change, not on every render.
  useEffect(() => {
    if (revision === 0) return;
    void fetchLayout();
  }, [revision, fetchLayout]);

  // Hide-system filter toggled: relayout with the new filter. (At mount
  // generation is 0 so fetchLayout no-ops; real toggles happen mid-scan.)
  useEffect(() => {
    void fetchLayout();
  }, [hideSystem, fetchLayout]);

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
      }
    } else if (prev === "scanning") {
      void fetchLayout();
    }
  }, [snapshot, generation, fetchLayout]);

  // Resize: mount-once observer; re-blit the old bitmap immediately so the
  // pane never goes blank while the relayout is fetched.
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
      blit();
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
  }, [blit, fetchLayout]);

  const hitTest = useCallback((cssX: number, cssY: number): TreemapRect | null => {
    if (hitFrozenRef.current) return null;
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

  /// The depth-1 directory under the point: what the zoom gestures target.
  const regionAt = useCallback((cssX: number, cssY: number): TreemapRect | null => {
    if (hitFrozenRef.current) return null;
    for (const r of rectsRef.current) {
      if (
        r.depth === 1 &&
        r.isDir &&
        cssX >= r.x &&
        cssX < r.x + r.w &&
        cssY >= r.y &&
        cssY < r.y + r.h
      ) {
        return r;
      }
    }
    return null;
  }, []);

  // Positions the tooltip at the last cursor position. Needed outside
  // mousemove too: the div mounts only after the debounce + fetch, usually
  // with the cursor already at rest — without a mount-time placement it
  // renders at its default (0,0), the pane's top-left corner.
  const placeTooltip = useCallback(() => {
    const container = containerRef.current;
    const tip = tooltipRef.current;
    if (!container || !tip) return;
    const bounds = container.getBoundingClientRect();
    const { x, y } = lastMouseRef.current;
    const tx = Math.min(x + 14, bounds.width - 260);
    const ty = Math.min(y + 16, bounds.height - 70);
    tip.style.transform = `translate(${Math.max(0, tx)}px, ${Math.max(0, ty)}px)`;
  }, []);

  useLayoutEffect(() => {
    if (tooltip) placeTooltip();
  }, [tooltip, placeTooltip]);

  const handleMove = useCallback(
    (e: React.MouseEvent) => {
      const container = containerRef.current;
      if (!container) return;
      const bounds = container.getBoundingClientRect();
      const x = e.clientX - bounds.left;
      const y = e.clientY - bounds.top;
      lastMouseRef.current = { x, y };
      placeTooltip();

      const hit = hitTest(x, y);
      const id = hit?.id ?? null;
      if (id === mouseOverRef.current) return;
      mouseOverRef.current = id;
      onHover(id);

      // Tooltip content only after the cursor settles — spam-hovering must
      // not flood IPC with getNode/getPath calls.
      window.clearTimeout(tooltipTimerRef.current);
      tooltipSeqRef.current++;
      if (id === null) {
        setTooltip(null);
        return;
      }
      setTooltip(null);
      const seq = tooltipSeqRef.current;
      tooltipTimerRef.current = window.setTimeout(() => {
        const generation = generationRef.current;
        if (generation === 0 || seq !== tooltipSeqRef.current) return;
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
          // Cosmetic path: a missed tooltip isn't worth a flash.
          .catch(() => {});
      }, TOOLTIP_DELAY_MS);
    },
    [hitTest, onHover, placeTooltip],
  );

  const handleLeave = useCallback(() => {
    mouseOverRef.current = null;
    tooltipSeqRef.current++;
    window.clearTimeout(tooltipTimerRef.current);
    setTooltip(null);
    onHover(null);
  }, [onHover]);

  const handleClick = useCallback(
    (e: React.MouseEvent) => {
      const bounds = containerRef.current!.getBoundingClientRect();
      const hit = hitTest(e.clientX - bounds.left, e.clientY - bounds.top);
      if (hit) onSelect(hit);
    },
    [hitTest, onSelect],
  );

  const handleContextMenu = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      const bounds = containerRef.current!.getBoundingClientRect();
      const hit = hitTest(e.clientX - bounds.left, e.clientY - bounds.top);
      if (hit) onContext(hit.id, e.clientX, e.clientY);
    },
    [hitTest, onContext],
  );

  // Zoom into the top-level folder under the cursor — works anywhere in its
  // region, no need to hit a (mostly covered) directory tile.
  const handleDoubleClick = useCallback(
    (e: React.MouseEvent) => {
      if (zoomRafRef.current !== 0) return;
      const bounds = containerRef.current!.getBoundingClientRect();
      const region = regionAt(e.clientX - bounds.left, e.clientY - bounds.top);
      if (region) onNavigate(region.id);
    },
    [regionAt, onNavigate],
  );

  const zoomOut = useCallback(() => {
    if (crumbs.length < 2 || hitFrozenRef.current) return;
    onNavigate(crumbs[crumbs.length - 2].id);
  }, [crumbs, onNavigate]);

  const handleWheel = useCallback(
    (e: React.WheelEvent) => {
      if (e.deltaY > 0) {
        zoomOut();
        return;
      }
      const bounds = containerRef.current!.getBoundingClientRect();
      const region = regionAt(e.clientX - bounds.left, e.clientY - bounds.top);
      if (region) onNavigate(region.id);
    },
    [zoomOut, regionAt, onNavigate],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) return;
      if (e.key === "Backspace" || (e.altKey && e.key === "ArrowUp")) {
        e.preventDefault();
        zoomOut();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [zoomOut]);

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
                onClick={() => onNavigate(c.id)}
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
        onContextMenu={handleContextMenu}
        onWheel={handleWheel}
      >
        <canvas ref={baseRef} className="absolute inset-0 h-full w-full" />
        <canvas ref={overlayRef} className="absolute inset-0 h-full w-full" />
        {!hasRects && (
          <div className="absolute inset-0 flex items-center justify-center text-xs text-zinc-600">
            {generation === 0
              ? "Treemap appears here during a scan"
              : "Waiting for data…"}
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
