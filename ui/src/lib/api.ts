// Typed surface of the Rust backend (src-tauri/src/scan.rs).

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type ScanState = "idle" | "scanning" | "done" | "cancelled" | "failed";

export interface Snapshot {
  generation: number;
  state: ScanState;
  files: number;
  dirs: number;
  bytes: number;
  errors: number;
  elapsedMs: number;
  nodes: number;
  rootError: string | null;
}

export interface Row {
  id: number;
  name: string;
  isDir: boolean;
  size: number;
  allocated: number;
  items: number;
  mtime: number;
  hasChildren: boolean;
  isReparse: boolean;
  isError: boolean;
  /** Fraction of the parent directory's total size, 0..=1. */
  pct: number;
}

export interface DirListing {
  id: number;
  rows: Row[];
}

export interface TreemapRect {
  id: number;
  x: number;
  y: number;
  w: number;
  h: number;
  depth: number;
  isDir: boolean;
  category: number;
}

export interface Crumb {
  id: number;
  name: string;
}

export type SortKey = "name" | "size" | "items" | "mtime";

export const api = {
  startScan: (path: string) => invoke<number>("start_scan", { path }),
  cancelScan: () => invoke<void>("cancel_scan"),
  scanStatus: () => invoke<Snapshot>("scan_status"),
  getChildren: (
    generation: number,
    ids: number[],
    sortBy: SortKey,
    descending: boolean,
  ) =>
    invoke<DirListing[]>("get_children", { generation, ids, sortBy, descending }),
  getNode: (generation: number, id: number) =>
    invoke<Row | null>("get_node", { generation, id }),
  getPath: (generation: number, id: number) =>
    invoke<string>("get_path", { generation, id }),
  getTreemap: (generation: number, rootId: number, width: number, height: number) =>
    invoke<TreemapRect[]>("get_treemap", { generation, rootId, width, height }),
  getAncestors: (generation: number, id: number) =>
    invoke<Crumb[]>("get_ancestors", { generation, id }),
};

export function onScanTick(cb: (s: Snapshot) => void): Promise<UnlistenFn> {
  return listen<Snapshot>("scan://tick", (e) => cb(e.payload));
}

export function onScanDone(cb: (s: Snapshot) => void): Promise<UnlistenFn> {
  return listen<Snapshot>("scan://done", (e) => cb(e.payload));
}
