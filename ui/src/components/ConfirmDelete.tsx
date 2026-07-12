import { useEffect, useRef, useState } from "react";
import { api, type DeletePreflight } from "../lib/api";
import { formatBytes, formatNumber } from "../lib/format";
import { reportUnlessStale } from "../lib/errors";

export interface DeleteTarget {
  id: number;
  name: string;
  isDir: boolean;
  size: number;
  items: number;
}

interface ConfirmDeleteProps {
  target: DeleteTarget;
  generation: number;
  permanent: boolean;
  busy: boolean;
  onPermanentChange: (v: boolean) => void;
  onCancel: () => void;
  onConfirm: () => void;
}

export function ConfirmDelete({
  target,
  generation,
  permanent,
  busy,
  onPermanentChange,
  onCancel,
  onConfirm,
}: ConfirmDeleteProps) {
  // Confirm stays disabled until the preflight lands: it carries the path
  // shown to the user and whether policy blocks this delete outright.
  const [preflight, setPreflight] = useState<DeletePreflight | null>(null);
  const checkboxRef = useRef<HTMLInputElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    cancelRef.current?.focus();
  }, []);

  // Arrows cycle the enabled controls; Enter/Space then act natively on
  // whichever is focused. Escape (below) and Tab stay untouched.
  const handleKeyDown = (e: React.KeyboardEvent) => {
    const forward = e.key === "ArrowRight" || e.key === "ArrowDown";
    const backward = e.key === "ArrowLeft" || e.key === "ArrowUp";
    if (!forward && !backward) return;
    e.preventDefault();
    const controls = [
      checkboxRef.current,
      cancelRef.current,
      confirmRef.current,
    ].filter(
      (el): el is HTMLInputElement | HTMLButtonElement =>
        el !== null && !el.disabled,
    );
    if (controls.length === 0) return;
    const i = controls.indexOf(document.activeElement as HTMLButtonElement);
    const next =
      i < 0
        ? controls[forward ? 0 : controls.length - 1]
        : controls[
            (i + (forward ? 1 : -1) + controls.length) % controls.length
          ];
    next.focus();
  };

  useEffect(() => {
    let live = true;
    api
      .deletePreflight(generation, target.id)
      .then((p) => {
        if (live) setPreflight(p);
      })
      .catch((e) => reportUnlessStale("resolving path", e));
    return () => {
      live = false;
    };
  }, [generation, target.id]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onCancel]);

  const kind = target.isDir ? "folder" : "file";
  const blocked = preflight?.blockReason ?? null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onMouseDown={() => {
        if (!busy) onCancel();
      }}
    >
      <div
        className="w-[420px] max-w-[92vw] rounded-lg border border-edge-strong bg-panel p-5 shadow-2xl"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <h2 className="text-sm font-semibold text-ink">Delete this {kind}?</h2>

        <div className="mt-3 rounded-md border border-edge bg-app/60 px-3 py-2">
          <div
            className="truncate text-[13px] font-medium text-ink"
            title={target.name}
          >
            {target.name}
          </div>
          <div
            className="mt-0.5 truncate text-[11px] text-ink-4"
            title={preflight?.path ?? undefined}
          >
            {preflight?.path ?? "…"}
          </div>
          <div className="tnum mt-1 text-[11px] text-ink-4">
            {formatBytes(target.size)}
            {target.isDir && ` · ${formatNumber(target.items)} items`}
          </div>
        </div>

        <label className="mt-3 flex cursor-pointer items-center gap-2 text-[12px] text-ink-3">
          <input
            ref={checkboxRef}
            type="checkbox"
            className="accent-danger"
            checked={permanent}
            disabled={busy || blocked !== null}
            onChange={(e) => onPermanentChange(e.target.checked)}
          />
          Delete permanently (skip the Recycle Bin)
        </label>

        <p
          className={`mt-2 text-[11px] ${
            blocked ? "text-warn" : permanent ? "text-danger-ink" : "text-ink-4"
          }`}
        >
          {blocked ??
            (permanent
              ? "This can't be undone."
              : "Moves to the Recycle Bin — you can restore it from there.")}
        </p>

        <div className="mt-4 flex justify-end gap-2">
          <button
            ref={cancelRef}
            className="rounded-md border border-edge-strong px-3 py-1.5 text-[13px] text-ink-2 hover:bg-raised disabled:opacity-50"
            disabled={busy}
            onClick={onCancel}
          >
            Cancel
          </button>
          <button
            ref={confirmRef}
            className={`rounded-md px-3 py-1.5 text-[13px] font-medium text-white disabled:opacity-60 ${
              permanent
                ? "bg-danger hover:bg-danger-hover"
                : "bg-accent hover:bg-accent-hover"
            }`}
            disabled={busy || preflight === null || blocked !== null}
            onClick={onConfirm}
          >
            {busy
              ? "Deleting…"
              : permanent
                ? "Delete permanently"
                : "Move to Recycle Bin"}
          </button>
        </div>
      </div>
    </div>
  );
}
