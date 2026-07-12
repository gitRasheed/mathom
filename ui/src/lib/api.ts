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

export interface DeletePreflight {
  path: string;
  /** Set when policy forbids deleting this path; the UI disables confirm. */
  blockReason: string | null;
}

export interface DeleteResult {
  removedBytes: number;
  removedFiles: number;
  removedDirs: number;
  parentId: number | null;
  trashed: boolean;
}

export type SortKey = "name" | "size" | "items" | "mtime";

export interface TypeStat {
  /** Lowercased extension; "" is the "no extension" group. */
  ext: string;
  /** Index into the shared category palette. */
  category: number;
  bytes: number;
  files: number;
}

export interface TypePanelData {
  /** Every extension in the subtree, largest first. */
  types: TypeStat[];
  totalBytes: number;
  totalFiles: number;
  topFiles: Row[];
}

export interface SearchHit {
  id: number;
  name: string;
  isDir: boolean;
  size: number;
  path: string;
}

export interface SearchResults {
  hits: SearchHit[];
  /** Every match, not just the returned top slice. */
  total: number;
}

export interface ElevationStatus {
  elevated: boolean;
  /** Dev builds can't usefully relaunch — the UI hides the button. */
  devBuild: boolean;
}

export interface DriveInfo {
  /** Root path as the scanner expects it, e.g. `C:\`. */
  path: string;
  /** Volume label; empty when the volume has none. */
  label: string;
  total: number;
  free: number;
}

export type ExportFormat = "csv" | "json";

export interface ExportArgs {
  maxDepth: number | null;
  dirsOnly: boolean;
  hideSystem: boolean;
}

export interface ExportText {
  rows: number;
  text: string;
}

export const api = {
  startScan: (path: string) => invoke<number>("start_scan", { path }),
  cancelScan: () => invoke<void>("cancel_scan"),
  scanStatus: () => invoke<Snapshot>("scan_status"),
  getChildren: (
    generation: number,
    ids: number[],
    sortBy: SortKey,
    descending: boolean,
    hideSystem: boolean,
    filter: string | null,
  ) =>
    invoke<DirListing[]>("get_children", {
      generation,
      ids,
      sortBy,
      descending,
      hideSystem,
      filter,
    }),
  getNode: (generation: number, id: number) =>
    invoke<Row | null>("get_node", { generation, id }),
  getPath: (generation: number, id: number) =>
    invoke<string>("get_path", { generation, id }),
  getTreemap: (
    generation: number,
    rootId: number,
    width: number,
    height: number,
    hideSystem: boolean,
    filter: string | null,
  ) =>
    invoke<TreemapRect[]>("get_treemap", {
      generation,
      rootId,
      width,
      height,
      hideSystem,
      filter,
    }),
  getTypeStats: (
    generation: number,
    rootId: number,
    hideSystem: boolean,
    filter: string | null,
  ) =>
    invoke<TypePanelData>("get_type_stats", {
      generation,
      rootId,
      hideSystem,
      filter,
    }),
  getAncestors: (generation: number, id: number) =>
    invoke<Crumb[]>("get_ancestors", { generation, id }),
  search: (generation: number, query: string, hideSystem: boolean) =>
    invoke<SearchResults>("search", { generation, query, hideSystem }),
  deletePreflight: (generation: number, id: number) =>
    invoke<DeletePreflight>("delete_preflight", { generation, id }),
  deleteEntry: (generation: number, id: number, permanent: boolean) =>
    invoke<DeleteResult>("delete_entry", { generation, id, permanent }),
  openInExplorer: (generation: number, id: number) =>
    invoke<void>("open_in_explorer", { generation, id }),
  exportTree: (
    generation: number,
    rootId: number,
    format: ExportFormat,
    dest: string,
    args: ExportArgs,
  ) =>
    invoke<number>("export_tree", { generation, rootId, format, dest, args }),
  exportText: (
    generation: number,
    rootId: number,
    format: ExportFormat,
    args: ExportArgs,
  ) => invoke<ExportText>("export_text", { generation, rootId, format, args }),
  listDrives: () => invoke<DriveInfo[]>("list_drives"),
  elevationStatus: () => invoke<ElevationStatus>("elevation_status"),
  /** Resolves only on decline (as an error); on success the app exits. */
  relaunchElevated: () => invoke<void>("relaunch_elevated"),
};

export function onScanTick(cb: (s: Snapshot) => void): Promise<UnlistenFn> {
  return listen<Snapshot>("scan://tick", (e) => cb(e.payload));
}

export function onScanDone(cb: (s: Snapshot) => void): Promise<UnlistenFn> {
  return listen<Snapshot>("scan://done", (e) => cb(e.payload));
}
