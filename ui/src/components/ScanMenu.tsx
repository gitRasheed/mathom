import { useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { api, type DriveInfo } from "../lib/api";
import { formatBytes } from "../lib/format";
import { ChevronIcon } from "./icons";

interface ScanMenuProps {
  onScan: (path: string) => void;
}

/** The one primary control: pick a drive (fetched fresh per open) or a folder. */
export function ScanMenu({ onScan }: ScanMenuProps) {
  const [open, setOpen] = useState(false);
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const boxRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    api
      .listDrives()
      .then(setDrives)
      .catch(() => setDrives([]));
    const onDown = (e: MouseEvent) => {
      if (!boxRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const scan = (path: string) => {
    setOpen(false);
    onScan(path);
  };

  const chooseFolder = async () => {
    setOpen(false);
    const picked = await openDialog({
      directory: true,
      title: "Choose a folder to scan",
    });
    if (typeof picked === "string") onScan(picked);
  };

  return (
    <div ref={boxRef} className="relative shrink-0">
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex h-8 items-center gap-1.5 rounded-md bg-accent px-4 text-[13px] font-medium text-white hover:bg-accent-hover"
      >
        Scan
        <ChevronIcon className="rotate-90" />
      </button>
      {open && (
        <div className="absolute top-9 left-0 z-50 w-80 rounded-md border border-edge-strong bg-panel py-1 shadow-xl">
          {drives.map((d) => {
            const used = d.total > 0 ? (d.total - d.free) / d.total : 0;
            return (
              <button
                key={d.path}
                onClick={() => scan(d.path)}
                title={`${formatBytes(d.total - d.free)} used of ${formatBytes(d.total)}`}
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[13px] text-ink hover:bg-raised"
              >
                <span className="w-7 shrink-0 font-medium">
                  {d.path.slice(0, 2)}
                </span>
                <span className="min-w-0 flex-1 truncate text-ink-3">
                  {d.label}
                </span>
                <span className="h-[5px] w-16 shrink-0 overflow-hidden rounded-sm bg-app">
                  <span
                    className="block h-full rounded-sm bg-accent/80"
                    style={{ width: `${Math.min(100, used * 100)}%` }}
                  />
                </span>
                <span className="tnum w-24 shrink-0 text-right text-[11px] text-ink-4">
                  {formatBytes(d.free)} free
                </span>
              </button>
            );
          })}
          {drives.length > 0 && (
            <div className="mx-1 my-1 border-t border-edge" />
          )}
          <button
            onClick={() => void chooseFolder()}
            className="flex w-full items-center px-3 py-1.5 text-left text-[13px] text-ink hover:bg-raised"
          >
            Choose folder…
          </button>
        </div>
      )}
    </div>
  );
}
