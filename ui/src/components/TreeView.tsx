import { useEffect, useMemo } from "react";
import { List, useListRef, type RowComponentProps } from "react-window";
import type { Row, SortKey } from "../lib/api";
import type { Sort } from "../hooks/useScan";
import {
  formatBytes,
  formatDate,
  formatNumber,
  formatPercent,
} from "../lib/format";
import { ChevronIcon } from "./icons";

const ROW_HEIGHT = 28;

// The name column flexes but never drops below NAME_MIN; trailing columns drop one by one as the pane narrows.
const NAME_MIN = 220;
const SIZE_W = 100;
const PCT_W = 136;
const ITEMS_W = 84;
const MTIME_W = 112;

interface ColumnPlan {
  template: string;
  pct: boolean;
  items: boolean;
  mtime: boolean;
}

function planColumns(width: number): ColumnPlan {
  const nameFits = (fixed: number) => width - fixed >= NAME_MIN;
  const pct = nameFits(SIZE_W + PCT_W);
  const items = pct && nameFits(SIZE_W + PCT_W + ITEMS_W);
  const mtime = items && nameFits(SIZE_W + PCT_W + ITEMS_W + MTIME_W);
  const template = [
    "minmax(0, 1fr)",
    `${SIZE_W}px`,
    ...(pct ? [`${PCT_W}px`] : []),
    ...(items ? [`${ITEMS_W}px`] : []),
    ...(mtime ? [`${MTIME_W}px`] : []),
  ].join(" ");
  return { template, pct, items, mtime };
}

interface FlatRow {
  row: Row;
  depth: number;
}

function flatten(
  root: Row,
  childrenMap: ReadonlyMap<number, Row[]>,
  expanded: ReadonlySet<number>,
): FlatRow[] {
  const out: FlatRow[] = [];
  const visit = (row: Row, depth: number) => {
    out.push({ row, depth });
    if (row.isDir && expanded.has(row.id)) {
      const kids = childrenMap.get(row.id);
      if (kids) for (const k of kids) visit(k, depth + 1);
    }
  };
  visit(root, 0);
  return out;
}

const NAV_KEYS = new Set([
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Enter",
]);

type NavAction =
  | { kind: "step"; index: number }
  | { kind: "toggle"; id: number }
  | { kind: "open"; row: Row };

function navAction(
  key: string,
  flat: FlatRow[],
  selectedIndex: number,
  expanded: ReadonlySet<number>,
): NavAction | null {
  if (selectedIndex < 0) {
    const wantsFirst = key === "ArrowDown" || key === "ArrowUp";
    return wantsFirst && flat.length > 0 ? { kind: "step", index: 0 } : null;
  }
  const { row, depth } = flat[selectedIndex];
  switch (key) {
    case "ArrowDown":
      return selectedIndex + 1 < flat.length
        ? { kind: "step", index: selectedIndex + 1 }
        : null;
    case "ArrowUp":
      return selectedIndex > 0
        ? { kind: "step", index: selectedIndex - 1 }
        : null;
    case "ArrowRight": {
      if (!row.isDir || !row.hasChildren) return null;
      if (!expanded.has(row.id)) return { kind: "toggle", id: row.id };
      const next = selectedIndex + 1;
      return next < flat.length && flat[next].depth > depth
        ? { kind: "step", index: next }
        : null;
    }
    case "ArrowLeft": {
      if (row.isDir && row.hasChildren && expanded.has(row.id)) {
        return { kind: "toggle", id: row.id };
      }
      for (let i = selectedIndex - 1; i >= 0; i--) {
        if (flat[i].depth < depth) return { kind: "step", index: i };
      }
      return null;
    }
    case "Enter":
      return { kind: "open", row };
    default:
      return null;
  }
}

interface TreeRowProps {
  flat: FlatRow[];
  cols: ColumnPlan;
  expanded: ReadonlySet<number>;
  selected: number | null;
  hoveredId: number | null;
  onToggle: (id: number) => void;
  onSelect: (row: Row) => void;
  onHoverRow: (id: number | null) => void;
  onContext: (id: number, x: number, y: number) => void;
}

function TreeRow({
  index,
  style,
  flat,
  cols,
  expanded,
  selected,
  hoveredId,
  onToggle,
  onSelect,
  onHoverRow,
  onContext,
}: RowComponentProps<TreeRowProps>) {
  const { row, depth } = flat[index];
  const isSelected = selected === row.id;
  const isHovered = hoveredId === row.id;

  return (
    <div
      style={{ ...style, display: "grid", gridTemplateColumns: cols.template }}
      className={`items-center text-[13px] ${
        isSelected
          ? "bg-raised/80"
          : isHovered
            ? "bg-accent-soft/25"
            : "hover:bg-hush"
      }`}
      onClick={() => onSelect(row)}
      onDoubleClick={() => {
        if (row.isDir && row.hasChildren) onToggle(row.id);
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        onContext(row.id, e.clientX, e.clientY);
      }}
      onMouseEnter={() => onHoverRow(row.id)}
    >
      <div
        className="flex min-w-0 items-center"
        style={{ paddingLeft: 8 + depth * 16 }}
      >
        {row.isDir && row.hasChildren ? (
          <button
            className="mr-1 flex h-4 w-4 shrink-0 items-center justify-center text-ink-4 hover:text-ink"
            onClick={(e) => {
              e.stopPropagation();
              onToggle(row.id);
            }}
          >
            <ChevronIcon
              className={expanded.has(row.id) ? "rotate-90" : ""}
              style={{ transition: "transform 120ms" }}
            />
          </button>
        ) : (
          <span className="mr-1 h-4 w-4 shrink-0" />
        )}
        <span
          className={`truncate ${row.isDir ? "text-ink" : "text-ink-3"}`}
          title={row.name}
        >
          {row.name}
        </span>
        {row.isReparse && (
          <span
            className="ml-1.5 shrink-0 text-[10px] text-ink-5"
            title="Junction / symlink — not followed"
          >
            link
          </span>
        )}
        {row.isError && (
          <span
            className="ml-1.5 shrink-0 text-[10px] text-danger-ink/80"
            title="Could not read this directory"
          >
            !
          </span>
        )}
      </div>
      <div className="tnum pr-3 text-right text-ink-2">
        {formatBytes(row.size)}
      </div>
      {cols.pct &&
        (depth === 0 ? (
          // "% of parent" is meaningless for the scan root itself.
          <div />
        ) : (
          <div className="flex items-center gap-2 pr-3">
            <div className="h-[5px] min-w-0 flex-1 overflow-hidden rounded-sm bg-raised">
              <div
                className="h-full rounded-sm bg-accent/80"
                style={{ width: `${Math.min(100, row.pct * 100)}%` }}
              />
            </div>
            <span className="tnum w-11 shrink-0 text-right text-[11px] text-ink-4">
              {formatPercent(row.pct)}
            </span>
          </div>
        ))}
      {cols.items && (
        <div className="tnum pr-3 text-right text-ink-4">
          {row.isDir ? formatNumber(row.items) : ""}
        </div>
      )}
      {cols.mtime && (
        <div className="tnum pr-3 text-right text-ink-4">
          {formatDate(row.mtime)}
        </div>
      )}
    </div>
  );
}

function Header({
  cols,
  sort,
  onSort,
}: {
  cols: ColumnPlan;
  sort: Sort;
  onSort: (k: SortKey) => void;
}) {
  const arrow = (key: SortKey) =>
    sort.key === key ? (
      <span className="ml-1 text-[9px]">{sort.desc ? "▼" : "▲"}</span>
    ) : null;
  const cls = (align: "left" | "right") =>
    `flex items-center py-1.5 text-[11px] font-medium uppercase tracking-wide text-ink-4 hover:text-ink-2 ${
      align === "right" ? "justify-end pr-3" : "pl-7"
    }`;

  return (
    <div
      className="border-b border-edge"
      style={{ display: "grid", gridTemplateColumns: cols.template }}
    >
      <button className={cls("left")} onClick={() => onSort("name")}>
        Name{arrow("name")}
      </button>
      <button className={cls("right")} onClick={() => onSort("size")}>
        Size{arrow("size")}
      </button>
      {cols.pct && (
        <span className="flex items-center justify-end py-1.5 pr-3 text-[11px] font-medium tracking-wide text-ink-5 uppercase">
          % of parent
        </span>
      )}
      {cols.items && (
        <button className={cls("right")} onClick={() => onSort("items")}>
          Items{arrow("items")}
        </button>
      )}
      {cols.mtime && (
        <button className={cls("right")} onClick={() => onSort("mtime")}>
          Modified{arrow("mtime")}
        </button>
      )}
    </div>
  );
}

export interface TreeViewProps {
  rootRow: Row;
  childrenMap: ReadonlyMap<number, Row[]>;
  expanded: ReadonlySet<number>;
  sort: Sort;
  /** Pane width in px — drives which columns fit (see planColumns). */
  width: number;
  selected: number | null;
  /** Node hovered in the treemap — highlighted here when visible. */
  hoveredId: number | null;
  /** Scroll this node into view once it appears in the flattened rows. */
  revealId: number | null;
  onRevealed: () => void;
  onToggle: (id: number) => void;
  onSelect: (row: Row) => void;
  /** Arrow-key selection: select only — Enter (onSelect) moves the view. */
  onKeySelect: (row: Row) => void;
  onHoverRow: (id: number | null) => void;
  onContext: (id: number, x: number, y: number) => void;
  onSort: (key: SortKey) => void;
}

export function TreeView({
  rootRow,
  childrenMap,
  expanded,
  sort,
  width,
  selected,
  hoveredId,
  revealId,
  onRevealed,
  onToggle,
  onSelect,
  onKeySelect,
  onHoverRow,
  onContext,
  onSort,
}: TreeViewProps) {
  const listRef = useListRef(null);
  const flat = useMemo(
    () => flatten(rootRow, childrenMap, expanded),
    [rootRow, childrenMap, expanded],
  );
  const cols = useMemo(() => planColumns(width), [width]);

  useEffect(() => {
    if (revealId === null) return;
    const index = flat.findIndex((f) => f.row.id === revealId);
    if (index >= 0) {
      listRef.current?.scrollToRow({ index, align: "smart", behavior: "auto" });
      onRevealed();
    }
    // else: listings are still loading; retry when `flat` changes.
  }, [revealId, flat, listRef, onRevealed]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    if (!NAV_KEYS.has(e.key)) return;
    e.preventDefault();
    const selectedIndex =
      selected === null ? -1 : flat.findIndex((f) => f.row.id === selected);
    const action = navAction(e.key, flat, selectedIndex, expanded);
    if (!action) return;
    if (action.kind === "step") {
      onKeySelect(flat[action.index].row);
      listRef.current?.scrollToRow({
        index: action.index,
        align: "smart",
        behavior: "auto",
      });
    } else if (action.kind === "toggle") {
      onToggle(action.id);
    } else {
      onSelect(action.row);
    }
  };

  return (
    <div
      className="flex min-h-0 flex-1 flex-col outline-none"
      tabIndex={0}
      onKeyDown={handleKeyDown}
    >
      <Header cols={cols} sort={sort} onSort={onSort} />
      <div className="min-h-0 flex-1" onMouseLeave={() => onHoverRow(null)}>
        <List
          listRef={listRef}
          rowComponent={TreeRow}
          rowCount={flat.length}
          rowHeight={ROW_HEIGHT}
          rowProps={{
            flat,
            cols,
            expanded,
            selected,
            hoveredId,
            onToggle,
            onSelect,
            onHoverRow,
            onContext,
          }}
          className="h-full"
        />
      </div>
    </div>
  );
}
