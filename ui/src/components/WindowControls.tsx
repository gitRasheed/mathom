import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { CloseIcon, MaximizeIcon, MinimizeIcon, RestoreIcon } from "./icons";

/** Minimize/maximize/close for the undecorated window; buttons stretch the toolbar's full height, flush with the edge. */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    const sync = () =>
      void win.isMaximized().then((v) => {
        setMaximized(v);
        document.documentElement.toggleAttribute("data-maximized", v);
      });
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
        <MinimizeIcon />
      </button>
      <button
        onClick={() => void win.toggleMaximize()}
        aria-label={maximized ? "Restore" : "Maximize"}
        className={cls}
      >
        {maximized ? <RestoreIcon /> : <MaximizeIcon />}
      </button>
      <button
        onClick={() => void win.close()}
        aria-label="Close"
        className="flex w-11 items-center justify-center text-ink-4 hover:bg-danger hover:text-white"
      >
        <CloseIcon />
      </button>
    </div>
  );
}
