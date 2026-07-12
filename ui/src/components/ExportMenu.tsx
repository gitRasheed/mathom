import { useEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api, type ExportFormat } from "../lib/api";
import { copyText } from "../lib/clipboard";
import { ExportIcon } from "./icons";

const DEPTHS = ["all", "1", "2"] as const;

interface ExportMenuProps {
  generation: number;
  viewRootId: number;
  hideSystem: boolean;
  disabled: boolean;
}

export function ExportMenu({
  generation,
  viewRootId,
  hideSystem,
  disabled,
}: ExportMenuProps) {
  const [open, setOpen] = useState(false);
  const [format, setFormat] = useState<ExportFormat>("csv");
  // "all", "1", "2", or whatever digits sit in the custom box.
  const [depth, setDepth] = useState("all");
  const [custom, setCustom] = useState("3");
  const [dirsOnly, setDirsOnly] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<{ ok: boolean; msg: string } | null>(
    null,
  );
  const boxRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
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

  const run = async (mode: "copy" | "save") => {
    setBusy(true);
    setStatus(null);
    const args = {
      maxDepth: depth === "all" ? null : Math.max(1, Number(depth) || 1),
      dirsOnly,
      hideSystem,
    };
    try {
      if (mode === "copy") {
        const res = await api.exportText(generation, viewRootId, format, args);
        const ok = await copyText(res.text);
        setStatus(
          ok
            ? { ok, msg: `Copied ${res.rows.toLocaleString()} rows` }
            : { ok, msg: "Clipboard refused the copy" },
        );
      } else {
        const dest = await save({
          defaultPath: `mathom-export.${format}`,
          filters: [{ name: format.toUpperCase(), extensions: [format] }],
        });
        if (typeof dest !== "string") return;
        const rows = await api.exportTree(
          generation,
          viewRootId,
          format,
          dest,
          args,
        );
        setStatus({ ok: true, msg: `Wrote ${rows.toLocaleString()} rows` });
      }
    } catch (e) {
      setStatus({ ok: false, msg: String(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div ref={boxRef} className="relative shrink-0">
      <button
        onClick={() => setOpen((v) => !v)}
        disabled={disabled}
        title={
          disabled
            ? "Export needs a finished scan"
            : "Export the current view as CSV or JSON"
        }
        aria-label="Export"
        className={`ml-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-md border disabled:cursor-not-allowed disabled:opacity-40 ${
          open
            ? "border-edge-strong bg-raised text-ink"
            : "border-edge bg-panel text-ink-4 hover:bg-raised hover:text-ink-2"
        }`}
      >
        <ExportIcon />
      </button>
      {open && (
        <div className="absolute top-9 right-0 z-50 w-56 rounded-md border border-edge-strong bg-panel p-3 shadow-xl">
          <div className="text-[11px] font-medium tracking-wide text-ink-4 uppercase">
            Format
          </div>
          <div className="mt-1.5 flex rounded-md border border-edge p-0.5">
            {(["csv", "json"] as const).map((f) => (
              <button
                key={f}
                onClick={() => setFormat(f)}
                className={`h-6 flex-1 rounded text-[12px] ${
                  format === f
                    ? "bg-raised text-ink"
                    : "text-ink-4 hover:text-ink-2"
                }`}
              >
                {f.toUpperCase()}
              </button>
            ))}
          </div>
          <div className="mt-3 text-[11px] font-medium tracking-wide text-ink-4 uppercase">
            Depth
          </div>
          <div className="mt-1.5 flex rounded-md border border-edge p-0.5">
            {DEPTHS.map((d) => (
              <button
                key={d}
                onClick={() => setDepth(d)}
                className={`h-6 flex-1 rounded text-[12px] ${
                  depth === d
                    ? "bg-raised text-ink"
                    : "text-ink-4 hover:text-ink-2"
                }`}
              >
                {d === "all" ? "All" : d}
              </button>
            ))}
            <input
              value={custom}
              onFocus={() => setDepth(custom)}
              onChange={(e) => {
                const v = e.target.value.replace(/\D/g, "").slice(0, 3);
                setCustom(v);
                setDepth(v);
              }}
              onBlur={() => {
                if (/^[1-9]\d{0,2}$/.test(custom)) return;
                setCustom("3");
                if (!DEPTHS.includes(depth as (typeof DEPTHS)[number])) {
                  setDepth("3");
                }
              }}
              placeholder="3+"
              title="Any depth — type a number"
              className={`h-6 w-0 min-w-0 flex-1 rounded text-center text-[12px] outline-none placeholder:text-ink-5 ${
                !DEPTHS.includes(depth as (typeof DEPTHS)[number])
                  ? "bg-raised text-ink"
                  : "bg-transparent text-ink-4 hover:text-ink-2"
              }`}
            />
          </div>
          <label className="mt-3 flex cursor-pointer items-center gap-1.5 text-[12px] text-ink-3 select-none">
            <input
              type="checkbox"
              className="accent-accent"
              checked={dirsOnly}
              onChange={(e) => setDirsOnly(e.target.checked)}
            />
            Folders only
          </label>
          <div className="mt-3 flex gap-2">
            <button
              onClick={() => void run("copy")}
              disabled={busy}
              className="h-7 flex-1 rounded-md border border-edge bg-panel text-[12px] text-ink-2 hover:bg-raised disabled:opacity-40"
            >
              Copy
            </button>
            <button
              onClick={() => void run("save")}
              disabled={busy}
              className="h-7 flex-1 rounded-md bg-accent text-[12px] font-medium text-white hover:bg-accent-hover disabled:opacity-40"
            >
              Save…
            </button>
          </div>
          {status && (
            <div
              className={`mt-2 truncate text-[11px] ${
                status.ok ? "text-ink-3" : "text-danger-ink"
              }`}
              title={status.msg}
            >
              {status.msg}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
