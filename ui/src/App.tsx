import { useCallback, useState } from "react";
import { StatusBar } from "./components/StatusBar";
import { Toolbar } from "./components/Toolbar";
import { TreeView } from "./components/TreeView";
import { useScan } from "./hooks/useScan";
import type { Snapshot } from "./lib/api";

export default function App() {
  const scan = useScan();
  const [selected, setSelected] = useState<number | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);

  const { pathOf, start } = scan;

  const handleSelect = useCallback(
    (id: number) => {
      setSelected(id);
      void pathOf(id).then(setSelectedPath);
    },
    [pathOf],
  );

  const handleScan = useCallback(
    (path: string) => {
      setSelected(null);
      setSelectedPath(null);
      void start(path);
    },
    [start],
  );

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
        <TreeView
          rootRow={scan.rootRow}
          childrenMap={scan.childrenMap}
          expanded={scan.expanded}
          sort={scan.sort}
          selected={selected}
          onToggle={scan.toggleExpand}
          onSelect={handleSelect}
          onSort={scan.changeSort}
        />
      ) : (
        <EmptyState snapshot={scan.snapshot} />
      )}
      <StatusBar snapshot={scan.snapshot} selectedPath={selectedPath} />
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
            The tree fills in live while the scan runs.
          </p>
        </div>
      )}
    </div>
  );
}
