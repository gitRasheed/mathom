// Client half of the snapshot protocol: the backend emits a throttled
// `scan://tick`, and this hook re-queries only the listings the UI can see
// (root + expanded directories). Every query carries the scan generation so
// results from a superseded scan are dropped.

import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  onScanDone,
  onScanTick,
  type DirListing,
  type Row,
  type Snapshot,
  type SortKey,
} from "../lib/api";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { reportUiError, reportUnlessStale } from "../lib/errors";

export interface Sort {
  key: SortKey;
  desc: boolean;
}

export interface ScanController {
  snapshot: Snapshot | null;
  generation: number;
  rootRow: Row | null;
  childrenMap: ReadonlyMap<number, Row[]>;
  expanded: ReadonlySet<number>;
  sort: Sort;
  hideSystem: boolean;
  startError: string | null;
  scanning: boolean;
  start: (path: string) => Promise<void>;
  cancel: () => void;
  toggleExpand: (id: number) => void;
  /** Expand every listed directory at once (treemap → tree reveal). */
  expandMany: (ids: number[]) => void;
  changeSort: (key: SortKey) => void;
  toggleHideSystem: () => void;
  pathOf: (id: number) => Promise<string | null>;
}

export function useScan(): ScanController {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [generation, setGeneration] = useState(0);
  const [rootRow, setRootRow] = useState<Row | null>(null);
  const [childrenMap, setChildrenMap] = useState<Map<number, Row[]>>(new Map());
  const [expanded, setExpanded] = useState<Set<number>>(new Set([0]));
  const [sort, setSort] = useState<Sort>({ key: "size", desc: true });
  const [hideSystem, setHideSystem] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);

  const genRef = useRef(0);
  const expandedRef = useRef(expanded);
  expandedRef.current = expanded;
  const sortRef = useRef(sort);
  sortRef.current = sort;
  const hideSystemRef = useRef(hideSystem);
  hideSystemRef.current = hideSystem;
  const busyRef = useRef(false);
  const queuedRef = useRef(false);

  const mergeListings = useCallback((listings: DirListing[]) => {
    if (listings.length === 0) return;
    setChildrenMap((prev) => {
      const next = new Map(prev);
      for (const l of listings) next.set(l.id, l.rows);
      return next;
    });
  }, []);

  // Coalesced: ticks arrive every ~100ms, but if a fetch is still in flight
  // we remember at most one follow-up instead of stacking invokes.
  const refresh = useCallback(async (): Promise<void> => {
    const gen = genRef.current;
    if (gen === 0) return;
    if (busyRef.current) {
      queuedRef.current = true;
      return;
    }
    busyRef.current = true;
    try {
      const s = sortRef.current;
      const ids = Array.from(new Set([0, ...expandedRef.current]));
      const [root, listings] = await Promise.all([
        api.getNode(gen, 0),
        api.getChildren(gen, ids, s.key, s.desc, hideSystemRef.current),
      ]);
      if (genRef.current === gen) {
        setRootRow(root);
        mergeListings(listings);
      }
    } catch (e) {
      reportUnlessStale("refreshing tree", e);
    } finally {
      busyRef.current = false;
      if (queuedRef.current) {
        queuedRef.current = false;
        void refresh();
      }
    }
  }, [mergeListings]);

  useEffect(() => {
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];
    const hook = (p: Promise<UnlistenFn>) => {
      void p.then((u) => {
        if (disposed) u();
        else unlisteners.push(u);
      });
    };
    const onSnapshot = (s: Snapshot) => {
      if (s.generation !== genRef.current) return;
      setSnapshot(s);
      void refresh();
    };
    hook(onScanTick(onSnapshot));
    hook(onScanDone(onSnapshot));
    return () => {
      disposed = true;
      for (const u of unlisteners) u();
    };
  }, [refresh]);

  // Adopt an already-running scan after a frontend reload (vite HMR in dev).
  useEffect(() => {
    api
      .scanStatus()
      .then((s) => {
        if (s.generation > 0 && genRef.current === 0) {
          genRef.current = s.generation;
          setGeneration(s.generation);
          setSnapshot(s);
          void refresh();
        }
      })
      .catch((e) => reportUiError("checking scan status", e));
  }, [refresh]);

  // Sort or the hide-system filter changed: refetch everything visible.
  useEffect(() => {
    void refresh();
  }, [sort, hideSystem, refresh]);

  const start = useCallback(
    async (path: string) => {
      setStartError(null);
      try {
        const gen = await api.startScan(path);
        genRef.current = gen;
        setGeneration(gen);
        setRootRow(null);
        setChildrenMap(new Map());
        setExpanded(new Set([0]));
        // A tiny scan can finish before the listener knows this generation;
        // pull the status once to close the gap.
        const s = await api.scanStatus();
        if (s.generation === gen) setSnapshot(s);
        void refresh();
      } catch (e) {
        setStartError(String(e));
      }
    },
    [refresh],
  );

  const cancel = useCallback(() => {
    void api.cancelScan().catch((e) => reportUiError("cancelling scan", e));
  }, []);

  const toggleExpand = useCallback(
    (id: number) => {
      const next = new Set(expandedRef.current);
      const nowExpanded = !next.has(id);
      if (nowExpanded) next.add(id);
      else next.delete(id);
      expandedRef.current = next;
      setExpanded(next);

      const gen = genRef.current;
      if (nowExpanded && gen !== 0) {
        const s = sortRef.current;
        api
          .getChildren(gen, [id], s.key, s.desc, hideSystemRef.current)
          .then((listings) => {
            if (genRef.current === gen) mergeListings(listings);
          })
          .catch((e) => reportUnlessStale("loading folder", e));
      }
    },
    [mergeListings],
  );

  const expandMany = useCallback(
    (ids: number[]) => {
      // Already-expanded dirs are kept fresh by refresh(); only fetch the
      // newly expanded ones so selection sync doesn't refetch the world.
      const fresh = ids.filter((id) => !expandedRef.current.has(id));
      if (fresh.length === 0) return;
      const next = new Set(expandedRef.current);
      for (const id of fresh) next.add(id);
      expandedRef.current = next;
      setExpanded(next);

      const gen = genRef.current;
      if (gen !== 0) {
        const s = sortRef.current;
        api
          .getChildren(gen, fresh, s.key, s.desc, hideSystemRef.current)
          .then((listings) => {
            if (genRef.current === gen) mergeListings(listings);
          })
          .catch((e) => reportUnlessStale("loading folders", e));
      }
    },
    [mergeListings],
  );

  const changeSort = useCallback((key: SortKey) => {
    setSort((prev) =>
      prev.key === key ? { key, desc: !prev.desc } : { key, desc: key !== "name" },
    );
  }, []);

  const toggleHideSystem = useCallback(() => setHideSystem((v) => !v), []);

  const pathOf = useCallback(async (id: number) => {
    const gen = genRef.current;
    if (gen === 0) return null;
    try {
      return await api.getPath(gen, id);
    } catch (e) {
      reportUnlessStale("resolving path", e);
      return null;
    }
  }, []);

  return {
    snapshot,
    generation,
    rootRow,
    childrenMap,
    expanded,
    sort,
    hideSystem,
    startError,
    scanning: snapshot?.state === "scanning",
    start,
    cancel,
    toggleExpand,
    expandMany,
    changeSort,
    toggleHideSystem,
    pathOf,
  };
}
