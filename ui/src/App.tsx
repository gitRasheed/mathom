import { useCallback, useEffect, useRef, useState } from "react";
import { StatusBar } from "./components/StatusBar";
import { Toolbar } from "./components/Toolbar";
import { TreeView } from "./components/TreeView";
import { Treemap } from "./components/Treemap";
import { useScan } from "./hooks/useScan";
import { api, type Row, type Snapshot, type TreemapRect } from "./lib/api";
import { onUiError, reportUnlessStale } from "./lib/errors";

const TREE_PANE_MIN = 320;
const TREEMAP_PANE_MIN = 280;

export default function App() {
  const scan = useScan();
  const [selected, setSelected] = useState<number | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  // What the treemap shows: the selected folder (or a file's parent folder).
  const [viewRootId, setViewRootId] = useState(0);
  const [hoveredId, setHoveredId] = useState<number | null>(null);
  const [revealId, setRevealId] = useState<number | null>(null);
  const [treeWidth, setTreeWidth] = useState(560);
  const [uiError, setUiError] = useState<string | null>(null);
  const splitRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let timer = 0;
    const off = onUiError((msg) => {
      setUiError(msg);
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setUiError(null), 8000);
    });
    return () => {
      window.clearTimeout(timer);
      off();
    };
  }, []);

  const { pathOf, start, expandMany, generation } = scan;

  const select = useCallback(
    (id: number) => {
      setSelected(id);
      void pathOf(id).then(setSelectedPath);
    },
    [pathOf],
  );

  // Tree click: a folder becomes the treemap view; a file shows its parent
  // folder with the file outlined.
  const handleTreeSelect = useCallback(
    (row: Row) => {
      select(row.id);
      if (row.isDir) {
        setViewRootId(row.id);
      } else if (generation !== 0) {
        api
          .getAncestors(generation, row.id)
          .then((chain) => {
            const parent = chain[chain.length - 2];
            if (parent) setViewRootId(parent.id);
          })
          .catch((e) => reportUnlessStale("locating parent", e));
      }
    },
    [select, generation],
  );

  // Treemap click: select + reveal in the tree. Folders (rarely hit — their
  // tiles are covered) also become the view; files never move the view.
  const handleTreemapSelect = useCallback(
    (rect: TreemapRect) => {
      select(rect.id);
      if (rect.isDir) setViewRootId(rect.id);
      if (generation === 0) return;
      api
        .getAncestors(generation, rect.id)
        .then((chain) => {
          expandMany(chain.slice(0, -1).map((c) => c.id));
          setRevealId(rect.id);
        })
        .catch((e) => reportUnlessStale("revealing selection", e));
    },
    [select, generation, expandMany],
  );

  // Breadcrumb / zoom gestures: pure navigation, selection stays put.
  const handleNavigate = useCallback((id: number) => {
    setViewRootId(id);
  }, []);

  const handleRevealed = useCallback(() => setRevealId(null), []);

  const handleScan = useCallback(
    (path: string) => {
      setSelected(null);
      setSelectedPath(null);
      setViewRootId(0);
      setHoveredId(null);
      setRevealId(null);
      void start(path);
    },
    [start],
  );

  const treeWidthRef = useRef(treeWidth);
  treeWidthRef.current = treeWidth;

  const startDivider = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = treeWidthRef.current;
    const total = splitRef.current?.clientWidth ?? window.innerWidth;
    const onMove = (ev: MouseEvent) => {
      const next = Math.min(
        Math.max(TREE_PANE_MIN, startWidth + ev.clientX - startX),
        total - TREEMAP_PANE_MIN,
      );
      setTreeWidth(next);
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }, []);

  return (
    <div className="flex h-full flex-col">
      <Toolbar
        scanning={scan.scanning}
        snapshot={scan.snapshot}
        startError={scan.startError}
        onScan={handleScan}
        onCancel={scan.cancel}
      />
      {scan.scanning ? <div className="scan-progress" /> : <div className="h-[2px]" />}
      {scan.rootRow ? (
        <div ref={splitRef} className="flex min-h-0 flex-1">
          <div
            className="flex min-h-0 flex-col"
            style={{ width: treeWidth, minWidth: TREE_PANE_MIN }}
          >
            <TreeView
              rootRow={scan.rootRow}
              childrenMap={scan.childrenMap}
              expanded={scan.expanded}
              sort={scan.sort}
              selected={selected}
              hoveredId={hoveredId}
              revealId={revealId}
              onRevealed={handleRevealed}
              onToggle={scan.toggleExpand}
              onSelect={handleTreeSelect}
              onHoverRow={setHoveredId}
              onSort={scan.changeSort}
            />
          </div>
          <div
            className="w-1 shrink-0 cursor-col-resize border-l border-zinc-800 hover:bg-teal-700/60"
            onMouseDown={startDivider}
          />
          <Treemap
            snapshot={scan.snapshot}
            generation={generation}
            rootId={viewRootId}
            selected={selected}
            hoveredId={hoveredId}
            onSelect={handleTreemapSelect}
            onHover={setHoveredId}
            onNavigate={handleNavigate}
          />
        </div>
      ) : (
        <EmptyState snapshot={scan.snapshot} />
      )}
      <StatusBar
        snapshot={scan.snapshot}
        selectedPath={selectedPath}
        uiError={uiError}
      />
    </div>
  );
}

function EmptyState({ snapshot }: { snapshot: Snapshot | null }) {
  const failed = snapshot?.state === "failed";
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-4">
      <div className="grid h-14 w-14 grid-cols-[1.4fr_1fr] grid-rows-[1.2fr_1fr] gap-1 opacity-80">
        <div className="row-span-2 rounded bg-teal-600" />
        <div className="rounded bg-teal-400" />
        <div className="rounded bg-teal-800" />
      </div>
      {failed ? (
        <p className="text-sm text-red-400">
          Scan failed: {snapshot?.rootError ?? "unknown error"}
        </p>
      ) : (
        <div className="text-center">
          <p className="text-sm text-zinc-400">Choose a folder and start a scan.</p>
          <p className="mt-1 text-xs text-zinc-600">
            The tree and treemap fill in live while the scan runs.
          </p>
        </div>
      )}
    </div>
  );
}
