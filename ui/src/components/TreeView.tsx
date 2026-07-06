import { useMemo } from "react";
import { List, type RowComponentProps } from "react-window";
import type { Row, SortKey } from "../lib/api";
import type { Sort } from "../hooks/useScan";
import {
  formatBytes,
  formatDate,
  formatNumber,
  formatPercent,
} from "../lib/format";

const GRID_COLS = "minmax(0, 1fr) 110px 150px 90px 130px";
const ROW_HEIGHT = 28;

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

interface TreeRowProps {
  flat: FlatRow[];
  expanded: ReadonlySet<number>;
  selected: number | null;
  onToggle: (id: number) => void;
  onSelect: (id: number) => void;
}

function TreeRow({
  index,
  style,
  flat,
  expanded,
  selected,
  onToggle,
  onSelect,
}: RowComponentProps<TreeRowProps>) {
  const { row, depth } = flat[index];
  const isSelected = selected === row.id;

  return (
    <div
      style={{ ...style, display: "grid", gridTemplateColumns: GRID_COLS }}
      className={`items-center text-[13px] ${
        isSelected ? "bg-zinc-800/80" : "hover:bg-zinc-900"
      }`}
      onClick={() => onSelect(row.id)}
      onDoubleClick={() => {
        if (row.isDir && row.hasChildren) onToggle(row.id);
      }}
    >
      <div
        className="flex min-w-0 items-center"
        style={{ paddingLeft: 8 + depth * 16 }}
      >
        {row.isDir && row.hasChildren ? (
          <button
            className="mr-1 flex h-4 w-4 shrink-0 items-center justify-center text-zinc-500 hover:text-zinc-200"
            onClick={(e) => {
              e.stopPropagation();
              onToggle(row.id);
            }}
          >
            <svg
              width="8"
              height="8"
              viewBox="0 0 8 8"
              className={expanded.has(row.id) ? "rotate-90" : ""}
              style={{ transition: "transform 120ms" }}
            >
              <path d="M2 0 L7 4 L2 8 Z" fill="currentColor" />
            </svg>
          </button>
        ) : (
          <span className="mr-1 h-4 w-4 shrink-0" />
        )}
        <span
          className={`truncate ${row.isDir ? "text-zinc-100" : "text-zinc-400"}`}
          title={row.name}
        >
          {row.name}
        </span>
        {row.isReparse && (
          <span
            className="ml-1.5 shrink-0 text-[10px] text-zinc-600"
            title="Junction / symlink — not followed"
          >
            link
          </span>
        )}
        {row.isError && (
          <span
            className="ml-1.5 shrink-0 text-[10px] text-red-400/80"
            title="Could not read this directory"
          >
            !
          </span>
        )}
      </div>
      <div className="tnum pr-3 text-right text-zinc-300">
        {formatBytes(row.size)}
      </div>
      <div className="flex items-center gap-2 pr-3">
        <div className="h-[5px] min-w-0 flex-1 overflow-hidden rounded-sm bg-zinc-800">
          <div
            className="h-full rounded-sm bg-teal-600/80"
            style={{ width: `${Math.min(100, row.pct * 100)}%` }}
          />
        </div>
        <span className="tnum w-11 shrink-0 text-right text-[11px] text-zinc-500">
          {formatPercent(row.pct)}
        </span>
      </div>
      <div className="tnum pr-3 text-right text-zinc-500">
        {row.isDir ? formatNumber(row.items) : ""}
      </div>
      <div className="tnum pr-3 text-right text-zinc-500">
        {formatDate(row.mtime)}
      </div>
    </div>
  );
}

function Header({ sort, onSort }: { sort: Sort; onSort: (k: SortKey) => void }) {
  const arrow = (key: SortKey) =>
    sort.key === key ? (
      <span className="ml-1 text-[9px]">{sort.desc ? "▼" : "▲"}</span>
    ) : null;
  const cls = (align: "left" | "right") =>
    `flex items-center py-1.5 text-[11px] font-medium uppercase tracking-wide text-zinc-500 hover:text-zinc-300 ${
      align === "right" ? "justify-end pr-3" : "pl-7"
    }`;

  return (
    <div
      className="border-b border-zinc-800"
      style={{ display: "grid", gridTemplateColumns: GRID_COLS }}
    >
      <button className={cls("left")} onClick={() => onSort("name")}>
        Name{arrow("name")}
      </button>
      <button className={cls("right")} onClick={() => onSort("size")}>
        Size{arrow("size")}
      </button>
      <span className="flex items-center justify-end py-1.5 pr-3 text-[11px] font-medium uppercase tracking-wide text-zinc-600">
        % of parent
      </span>
      <button className={cls("right")} onClick={() => onSort("items")}>
        Items{arrow("items")}
      </button>
      <button className={cls("right")} onClick={() => onSort("mtime")}>
        Modified{arrow("mtime")}
      </button>
    </div>
  );
}

export interface TreeViewProps {
  rootRow: Row;
  childrenMap: ReadonlyMap<number, Row[]>;
  expanded: ReadonlySet<number>;
  sort: Sort;
  selected: number | null;
  onToggle: (id: number) => void;
  onSelect: (id: number) => void;
  onSort: (key: SortKey) => void;
}

export function TreeView({
  rootRow,
  childrenMap,
  expanded,
  sort,
  selected,
  onToggle,
  onSelect,
  onSort,
}: TreeViewProps) {
  const flat = useMemo(
    () => flatten(rootRow, childrenMap, expanded),
    [rootRow, childrenMap, expanded],
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <Header sort={sort} onSort={onSort} />
      <div className="min-h-0 flex-1">
        <List
          rowComponent={TreeRow}
          rowCount={flat.length}
          rowHeight={ROW_HEIGHT}
          rowProps={{ flat, expanded, selected, onToggle, onSelect }}
          className="h-full"
        />
      </div>
    </div>
  );
}
