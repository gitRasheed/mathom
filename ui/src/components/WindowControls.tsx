import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

/** Minimize / maximize / close for the undecorated window; the buttons
 *  stretch to the toolbar's full height and sit flush with the edge. */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    const sync = () => void win.isMaximized().then(setMaximized);
    sync();
    void win.onResized(sync).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, []);

  const win = getCurrentWindow();
  const cls =
    "flex w-11 items-center justify-center text-ink-4 hover:bg-hush hover:text-ink";

  return (
    <div className="-my-2 -mr-3 ml-1 flex shrink-0 self-stretch">
      <button
        onClick={() => void win.minimize()}
        aria-label="Minimize"
        className={cls}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" stroke="currentColor">
          <line x1="0" y1="5" x2="10" y2="5" />
        </svg>
      </button>
      <button
        onClick={() => void win.toggleMaximize()}
        aria-label={maximized ? "Restore" : "Maximize"}
        className={cls}
      >
        {maximized ? (
          <svg
            width="10"
            height="10"
            viewBox="0 0 10 10"
            fill="none"
            stroke="currentColor"
          >
            <rect x="0.5" y="2.5" width="7" height="7" />
            <path d="M2.5 2.5 v-2 h7 v7 h-2" />
          </svg>
        ) : (
          <svg
            width="10"
            height="10"
            viewBox="0 0 10 10"
            fill="none"
            stroke="currentColor"
          >
            <rect x="0.5" y="0.5" width="9" height="9" />
          </svg>
        )}
      </button>
      <button
        onClick={() => void win.close()}
        aria-label="Close"
        className="flex w-11 items-center justify-center text-ink-4 hover:bg-danger hover:text-white"
      >
        <svg width="10" height="10" viewBox="0 0 10 10" stroke="currentColor">
          <path d="M0 0 L10 10 M10 0 L0 10" />
        </svg>
      </button>
    </div>
  );
}
