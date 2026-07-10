// Canvas treemap. Bake layout once, draw hover/selection on an overlay.
// The rect list arrives parents-before-children: forward = paint order,
// reverse = deepest-first hit-testing.
//
// INVARIANT: pipeline callbacks stay identity-stable; prop values are
// mirrored into refs, and effects depend only on values whose change
// genuinely requires them. A hover-dependent callback once cascaded into
// the reset/resize effects — every hover refetched the layout (IPC flood,
// webview crash).

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
import { PALETTE, canvasColors } from "../lib/palette";

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
  rootId: number;
  /** Bumped by App on an out-of-band tree change (a delete) to force relayout. */
  revision: number;
  /** Bumped by useTheme after a theme/accent change to force a re-bake. */
  themeRev: number;
  hideSystem: boolean;
  selected: number | null;
  hoveredId: number | null;
  onSelect: (rect: TreemapRect) => void;
  onHover: (id: number | null) => void;
  onNavigate: (id: number) => void;
  onContext: (id: number, x: number, y: number) => void;
}

export function Treemap({
  snapshot,
  generation,
  rootId,
  revision,
  themeRev,
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
  // During drill, old rects are stale geometry.
  const hitFrozenRef = useRef(false);
  const crumbsRootRef = useRef<number | null>(null);
  const crumbIdsRef = useRef<Set<number>>(new Set());
  const lastFetchRef = useRef(0);
  const fetchSeqRef = useRef(0);
  const zoomRafRef = useRef(0);
  const mouseOverRef = useRef<number | null>(null);
  const tooltipSeqRef = useRef(0);
  const tooltipTimerRef = useRef(0);
  const lastMouseRef = useRef({ x: 0, y: 0 });

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
    if (hitFrozenRef.current) return;
    const dpr = window.devicePixelRatio || 1;
    const theme = canvasColors();
    const outline = (id: number | null, color: string, width: number) => {
      if (id === null || id === rootIdRef.current) return;
      const r = byIdRef.current.get(id);
      if (!r) return;
      const s = snap(r, dpr, 0);
      ctx.strokeStyle = color;
      ctx.lineWidth = width;
      ctx.strokeRect(
        s.x + width / 2,
        s.y + width / 2,
        s.w - width,
        s.h - width,
      );
    };
    outline(selectedRef.current, theme.selection, 2);
    const hovered = hoveredIdRef.current;
    if (hovered !== null && hovered !== selectedRef.current) {
      if (hovered === rootIdRef.current || crumbIdsRef.current.has(hovered)) {
        ctx.strokeStyle = theme.hoverRing;
        ctx.lineWidth = 3;
        ctx.strokeRect(1.5, 1.5, overlay.width - 3, overlay.height - 3);
      } else {
        outline(hovered, theme.hoverRing, 2);
      }
    }
  }, []);

  const blit = useCallback(() => {
    const base = baseRef.current;
    const off = offscreenRef.current;
    if (!base || !off || off.width === 0 || off.height === 0) return;
    const ctx = base.getContext("2d")!;
    ctx.clearRect(0, 0, base.width, base.height);
    ctx.drawImage(
      off,
      0,
      0,
      off.width,
      off.height,
      0,
      0,
      base.width,
      base.height,
    );
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
    const theme = canvasColors();

    ctx.fillStyle = theme.background;
    ctx.fillRect(0, 0, off.width, off.height);

    ctx.fillStyle = theme.plate;
    ctx.beginPath();
    for (const r of rects) {
      if (!r.isDir) continue;
      const s = snap(r, dpr, 0);
      if (s.w > 0 && s.h > 0) ctx.rect(s.x, s.y, s.w, s.h);
    }
    ctx.fill();

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
        crumbIdsRef.current = new Set(crumbs.map((c) => c.id));
        setCrumbs(crumbs);
        drawOverlay(); // the current hover may have just become an ancestor
      })
      .catch((e) => {
        // The tree can still be empty at scan start; layout retries crumbs.
        if (!isStale(e) && !String(e).includes("unknown node")) {
          reportUnlessStale("loading breadcrumbs", e);
        }
      });
  }, [drawOverlay]);

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
      const rects = await api.getTreemap(
        generation,
        forRoot,
        w,
        h,
        hideSystemRef.current,
      );
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
      drawOverlay(); // clear rings: they describe the view being left

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
    [blit, drawOverlay, fetchLayout],
  );

  useEffect(() => {
    drillTo(rootId);
  }, [rootId, drillTo]);

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
    crumbIdsRef.current = new Set();
    offscreenRef.current = null;
    const base = baseRef.current;
    if (base) base.getContext("2d")!.clearRect(0, 0, base.width, base.height);
    if (generation !== 0) void fetchLayout();
  }, [generation, fetchLayout]);

  useEffect(() => {
    drawOverlay();
  }, [selected, hoveredId, drawOverlay]);

  useEffect(() => {
    if (revision === 0) return;
    void fetchLayout();
  }, [revision, fetchLayout]);

  useEffect(() => {
    bake(); // repaint the baked layout with the new theme's canvas colors
  }, [themeRev, bake]);

  useEffect(() => {
    void fetchLayout();
  }, [hideSystem, fetchLayout]);

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

  const hitTest = useCallback(
    (cssX: number, cssY: number): TreemapRect | null => {
      if (hitFrozenRef.current) return null;
      const rects = rectsRef.current;
      for (let i = rects.length - 1; i >= 0; i--) {
        const r = rects[i];
        if (
          cssX >= r.x &&
          cssX < r.x + r.w &&
          cssY >= r.y &&
          cssY < r.y + r.h
        ) {
          return r;
        }
      }
      return null;
    },
    [],
  );

  const regionAt = useCallback(
    (cssX: number, cssY: number): TreemapRect | null => {
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
    },
    [],
  );

  // Also called at mount: the div appears after the debounce with the cursor
  // already at rest, and would otherwise render at (0,0).
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
      <div className="flex h-8 shrink-0 items-center gap-1 overflow-hidden border-b border-edge px-3 text-xs">
        {crumbs.length === 0 ? (
          <span className="text-ink-5">Treemap</span>
        ) : (
          crumbs.map((c, i) => (
            <Fragment key={c.id}>
              {i > 0 && <span className="shrink-0 text-ink-5">›</span>}
              <button
                className={`max-w-56 truncate ${
                  i === crumbs.length - 1
                    ? "text-ink"
                    : "text-ink-4 hover:text-ink"
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
          <div className="absolute inset-0 flex items-center justify-center text-xs text-ink-5">
            {generation === 0
              ? "Treemap appears here during a scan"
              : "Waiting for data…"}
          </div>
        )}
        {tooltip && (
          <div
            ref={tooltipRef}
            className="pointer-events-none absolute top-0 left-0 z-10 max-w-64 rounded-md border border-edge-strong bg-panel/95 px-2.5 py-1.5 text-xs shadow-lg"
          >
            <div className="truncate font-medium text-ink">{tooltip.name}</div>
            <div className="tnum text-ink-3">
              {tooltip.size} · {tooltip.pct} of parent
            </div>
            <div className="truncate text-[11px] text-ink-4">
              {tooltip.path}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
