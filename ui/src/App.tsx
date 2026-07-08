import { useCallback, useEffect, useRef, useState } from "react";
import { ConfirmDelete, type DeleteTarget } from "./components/ConfirmDelete";
import { ContextMenu, type MenuItem } from "./components/ContextMenu";
import { StatusBar } from "./components/StatusBar";
import { Toolbar } from "./components/Toolbar";
import { TreeView } from "./components/TreeView";
import { Treemap } from "./components/Treemap";
import { TypePanel } from "./components/TypePanel";
import { useScan } from "./hooks/useScan";
import {
  api,
  type ElevationStatus,
  type Row,
  type SearchHit,
  type Snapshot,
  type TreemapRect,
} from "./lib/api";
import { copyText } from "./lib/clipboard";
import { onUiError, reportUiError, reportUnlessStale } from "./lib/errors";

const TREE_PANE_MIN = 320;
const TREEMAP_PANE_MIN = 280;

const targetFrom = (r: Row): DeleteTarget => ({
  id: r.id,
  name: r.name,
  isDir: r.isDir,
  size: r.size,
  items: r.items,
});

export default function App() {
  const scan = useScan();
  const [selected, setSelected] = useState<number | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [viewRootId, setViewRootId] = useState(0);
  const [hoveredId, setHoveredId] = useState<number | null>(null);
  const [revealId, setRevealId] = useState<number | null>(null);
  const [treeWidth, setTreeWidth] = useState(560);
  const [typePanelOpen, setTypePanelOpen] = useState(true);
  const [uiError, setUiError] = useState<string | null>(null);
  const [menu, setMenu] = useState<{
    x: number;
    y: number;
    target: DeleteTarget;
  } | null>(null);
  const [confirm, setConfirm] = useState<{
    target: DeleteTarget;
    permanent: boolean;
  } | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [treeRevision, setTreeRevision] = useState(0);
  const [elevation, setElevation] = useState<ElevationStatus | null>(null);
  const [elevationDismissed, setElevationDismissed] = useState(false);
  const splitRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api
      .elevationStatus()
      .then(setElevation)
      .catch(() => setElevation(null)); // unknown — don't nag
  }, []);

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

  const handlePanelFileSelect = useCallback(
    (row: Row) => {
      select(row.id);
      if (generation === 0) return;
      api
        .getAncestors(generation, row.id)
        .then((chain) => {
          expandMany(chain.slice(0, -1).map((c) => c.id));
          setRevealId(row.id);
        })
        .catch((e) => reportUnlessStale("revealing selection", e));
    },
    [select, generation, expandMany],
  );

  const handleSearchSelect = useCallback(
    (hit: SearchHit) => {
      select(hit.id);
      if (generation === 0) return;
      api
        .getAncestors(generation, hit.id)
        .then((chain) => {
          expandMany(chain.slice(0, -1).map((c) => c.id));
          setRevealId(hit.id);
          if (hit.isDir) {
            setViewRootId(hit.id);
          } else {
            const parent = chain[chain.length - 2];
            if (parent) setViewRootId(parent.id);
          }
        })
        .catch((e) => reportUnlessStale("revealing search result", e));
    },
    [select, generation, expandMany],
  );

  const handleNavigate = useCallback((id: number) => {
    setViewRootId(id);
  }, []);

  const handleRevealed = useCallback(() => setRevealId(null), []);

  const handleContextMenu = useCallback(
    (id: number, x: number, y: number) => {
      if (generation === 0 || id === 0) return;
      select(id);
      api
        .getNode(generation, id)
        .then((node) => {
          if (node) setMenu({ x, y, target: targetFrom(node) });
        })
        .catch((e) => reportUnlessStale("opening menu", e));
    },
    [generation, select],
  );

  const copyPath = useCallback(
    (id: number) => {
      if (generation === 0) return;
      api
        .getPath(generation, id)
        .then((p) => copyText(p))
        .catch((e) => reportUnlessStale("copying path", e));
    },
    [generation],
  );

  const openConfirm = useCallback(
    (id: number, permanent: boolean) => {
      if (generation === 0 || id === 0) return;
      api
        .getNode(generation, id)
        .then((node) => {
          if (node) setConfirm({ target: targetFrom(node), permanent });
        })
        .catch((e) => reportUnlessStale("preparing delete", e));
    },
    [generation],
  );

  const performDelete = useCallback(async () => {
    if (!confirm) return;
    const { target, permanent } = confirm;
    setDeleteBusy(true);
    try {
      const res = await api.deleteEntry(generation, target.id, permanent);
      setConfirm(null);
      if (selected === target.id) {
        setSelected(null);
        setSelectedPath(null);
      }
      if (res.parentId != null) {
        setViewRootId((vr) => (vr === target.id ? res.parentId! : vr));
      }
      setTreeRevision((r) => r + 1);
    } catch (e) {
      reportUiError("deleting", e);
    } finally {
      setDeleteBusy(false);
    }
  }, [confirm, generation, selected]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Delete" || confirm) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA")) return;
      if (selected == null || selected === 0) return;
      e.preventDefault();
      openConfirm(selected, e.shiftKey);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selected, confirm, openConfirm]);

  const handleScan = useCallback(
    (path: string) => {
      setSelected(null);
      setSelectedPath(null);
      setViewRootId(0);
      setHoveredId(null);
      setRevealId(null);
      setMenu(null);
      setConfirm(null);
      void start(path);
    },
    [start],
  );

  const menuItems: MenuItem[] = menu
    ? [
        {
          label: "Open in Explorer",
          onClick: () =>
            void api
              .openInExplorer(generation, menu.target.id)
              .catch((e) => reportUiError("opening in Explorer", e)),
        },
        { label: "Copy path", onClick: () => copyPath(menu.target.id) },
        {
          label: menu.target.isDir ? "Delete folder…" : "Delete file…",
          danger: true,
          onClick: () => setConfirm({ target: menu.target, permanent: false }),
        },
      ]
    : [];

  const treeWidthRef = useRef(treeWidth);
  treeWidthRef.current = treeWidth;
  const typePanelOpenRef = useRef(typePanelOpen);
  typePanelOpenRef.current = typePanelOpen;

  const startDivider = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = treeWidthRef.current;
    const total = splitRef.current?.clientWidth ?? window.innerWidth;
    // 256 = the type panel's fixed width (w-64) when open.
    const reserved = TREEMAP_PANE_MIN + (typePanelOpenRef.current ? 256 : 0);
    const onMove = (ev: MouseEvent) => {
      const next = Math.min(
        Math.max(TREE_PANE_MIN, startWidth + ev.clientX - startX),
        total - reserved,
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
        generation={generation}
        startError={scan.startError}
        hideSystem={scan.hideSystem}
        typePanelOpen={typePanelOpen}
        onScan={handleScan}
        onCancel={scan.cancel}
        onToggleHideSystem={scan.toggleHideSystem}
        onToggleTypePanel={() => setTypePanelOpen((v) => !v)}
        onSearchSelect={handleSearchSelect}
      />
      {elevation !== null && !elevation.elevated && !elevationDismissed && (
        <ElevationBanner
          devBuild={elevation.devBuild}
          onRelaunch={() =>
            void api
              .relaunchElevated()
              .catch((e) => reportUiError("relaunching as administrator", e))
          }
          onDismiss={() => setElevationDismissed(true)}
        />
      )}
      {scan.scanning ? (
        <div className="scan-progress" />
      ) : (
        <div className="h-[2px]" />
      )}
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
              width={treeWidth}
              selected={selected}
              hoveredId={hoveredId}
              revealId={revealId}
              onRevealed={handleRevealed}
              onToggle={scan.toggleExpand}
              onSelect={handleTreeSelect}
              onHoverRow={setHoveredId}
              onContext={handleContextMenu}
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
            revision={treeRevision}
            hideSystem={scan.hideSystem}
            selected={selected}
            hoveredId={hoveredId}
            onSelect={handleTreemapSelect}
            onHover={setHoveredId}
            onNavigate={handleNavigate}
            onContext={handleContextMenu}
          />
          {typePanelOpen && (
            <TypePanel
              snapshot={scan.snapshot}
              generation={generation}
              rootId={viewRootId}
              revision={treeRevision}
              hideSystem={scan.hideSystem}
              onSelectFile={handlePanelFileSelect}
            />
          )}
        </div>
      ) : (
        <EmptyState snapshot={scan.snapshot} />
      )}
      <StatusBar
        snapshot={scan.snapshot}
        selectedPath={selectedPath}
        uiError={uiError}
      />
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={menuItems}
          onClose={() => setMenu(null)}
        />
      )}
      {confirm && (
        <ConfirmDelete
          target={confirm.target}
          generation={generation}
          permanent={confirm.permanent}
          busy={deleteBusy}
          onPermanentChange={(v) =>
            setConfirm((c) => (c ? { ...c, permanent: v } : c))
          }
          onCancel={() => {
            if (!deleteBusy) setConfirm(null);
          }}
          onConfirm={() => void performDelete()}
        />
      )}
    </div>
  );
}

function ElevationBanner({
  devBuild,
  onRelaunch,
  onDismiss,
}: {
  devBuild: boolean;
  onRelaunch: () => void;
  onDismiss: () => void;
}) {
  return (
    <div className="flex shrink-0 items-center gap-3 border-b border-zinc-800 bg-zinc-900/70 px-3 py-1.5 text-xs">
      <span className="min-w-0 truncate text-zinc-400">
        Running without administrator rights — scans use the slower folder
        walker and skip files it can't read.
        {devBuild && " Start the dev loop from an elevated terminal instead."}
      </span>
      {!devBuild && (
        <button
          className="shrink-0 rounded border border-teal-700/60 px-2 py-0.5 text-teal-300 hover:bg-teal-900/40"
          onClick={onRelaunch}
        >
          Relaunch as administrator
        </button>
      )}
      <button
        className="ml-auto shrink-0 px-1 text-zinc-600 hover:text-zinc-300"
        onClick={onDismiss}
        aria-label="Dismiss"
      >
        ✕
      </button>
    </div>
  );
}

function EmptyState({ snapshot }: { snapshot: Snapshot | null }) {
  const failed = snapshot?.state === "failed";
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center">
      {failed ? (
        <p className="text-sm text-red-400">
          Scan failed: {snapshot?.rootError ?? "unknown error"}
        </p>
      ) : (
        <div className="text-center">
          <p className="text-sm text-zinc-400">
            Choose a folder and start a scan.
          </p>
          <p className="mt-1.5 text-xs text-zinc-600">
            The tree and treemap fill in live while the scan runs.
          </p>
        </div>
      )}
    </div>
  );
}
