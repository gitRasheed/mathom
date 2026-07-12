// Raw query box; grammar lives in mathom-core/src/search.rs.

import { useCallback, useEffect, useRef, useState } from "react";
import { api, type SearchHit, type SearchResults } from "../lib/api";
import { reportUnlessStale } from "../lib/errors";
import { formatBytes, formatNumber } from "../lib/format";

const DEBOUNCE_MS = 150;

// Rotating placeholder: the grammar teaches itself, no docs required.
const HINTS = [
  "Search names…",
  "Try ext:mp4 — filter by type",
  "Try >500mb — filter by size",
  "Enter filters the whole view",
];
const HINT_MS = 7000;

interface SearchBoxProps {
  generation: number;
  hideSystem: boolean;
  /** The query currently filtering all views, or null. */
  activeFilter: string | null;
  /** Filtering needs a finished scan. */
  canFilter: boolean;
  onSelect: (hit: SearchHit) => void;
  onApplyFilter: (query: string | null) => void;
}

function parentDir(path: string): string {
  const i = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return i > 0 ? path.slice(0, i) : path;
}

export function SearchBox({
  generation,
  hideSystem,
  activeFilter,
  canFilter,
  onSelect,
  onApplyFilter,
}: SearchBoxProps) {
  const [text, setText] = useState("");
  const [results, setResults] = useState<SearchResults | null>(null);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  // Plain Enter filters the view; Enter after ↑↓ opens the picked match.
  const [navigated, setNavigated] = useState(false);
  const [hint, setHint] = useState(0);
  const seqRef = useRef(0);
  const boxRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Filters can be set from outside (type-panel clicks) — the box adopts
  // the query text so it never lies, but silently: adopted text must not
  // pop the results dropdown the way typed text does. Never stomp typing.
  const adoptedRef = useRef(false);
  useEffect(() => {
    if (activeFilter === null) return;
    if (document.activeElement === inputRef.current) return;
    adoptedRef.current = true;
    setText(activeFilter);
  }, [activeFilter]);

  useEffect(() => {
    const t = window.setInterval(() => {
      // Rotating under the user's caret is disorienting — hold while focused.
      if (document.activeElement === inputRef.current) return;
      setHint((h) => (h + 1) % HINTS.length);
    }, HINT_MS);
    return () => window.clearInterval(t);
  }, []);

  useEffect(() => {
    const seq = ++seqRef.current;
    if (generation === 0 || text.trim() === "" || adoptedRef.current) {
      adoptedRef.current = false;
      setResults(null);
      setOpen(false);
      return;
    }
    const timer = window.setTimeout(() => {
      api
        .search(generation, text, hideSystem)
        .then((r) => {
          if (seq !== seqRef.current) return;
          setResults(r);
          setActive(0);
          setNavigated(false);
          setOpen(true);
        })
        .catch((e) => reportUnlessStale("searching", e));
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [text, generation, hideSystem]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!boxRef.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  useEffect(() => {
    boxRef.current
      ?.querySelector(`[data-i="${active}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const choose = useCallback(
    (hit: SearchHit) => {
      // A jump targets the real tree: when the box query isn't the applied
      // filter, the hit may be hidden by it — drop the filter first. Same
      // query = the hit is visible by construction, keep filtering.
      if (activeFilter && text.trim() !== activeFilter) onApplyFilter(null);
      onSelect(hit);
      setOpen(false);
    },
    [onSelect, onApplyFilter, activeFilter, text],
  );

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      if (open) {
        setOpen(false);
      } else if (text !== "") {
        setText("");
      } else if (activeFilter) {
        onApplyFilter(null);
      } else {
        inputRef.current?.blur();
      }
      return;
    }
    const hits = results?.hits ?? [];
    if (e.key === "ArrowDown" && open && hits.length > 0) {
      e.preventDefault();
      setNavigated(true);
      setActive((a) => Math.min(a + 1, hits.length - 1));
    } else if (e.key === "ArrowUp" && open && hits.length > 0) {
      e.preventDefault();
      setNavigated(true);
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === "Enter") {
      if (navigated && open && hits.length > 0) {
        const hit = hits[active] ?? hits[0];
        if (hit) choose(hit);
      } else if (canFilter && text.trim() !== "") {
        onApplyFilter(text.trim());
        setOpen(false);
      } else if (canFilter && text.trim() === "") {
        // Enter always applies the box's query; an empty query is no filter.
        onApplyFilter(null);
      } else if (open && hits.length > 0) {
        // Mid-scan there is no filtering; Enter opens the top match.
        choose(hits[active] ?? hits[0]);
      }
    }
  };

  return (
    <div ref={boxRef} className="relative">
      <input
        ref={inputRef}
        value={text}
        onChange={(e) => {
          adoptedRef.current = false;
          setText(e.target.value);
        }}
        onFocus={() => {
          if (results && text.trim() !== "") setOpen(true);
        }}
        onKeyDown={onKeyDown}
        placeholder={HINTS[hint]}
        spellCheck={false}
        disabled={generation === 0}
        title={
          "Space-separated filters, all must match:\nname substring · ext:mp4 · >100mb" +
          (canFilter ? "\nEnter filters every view; Esc clears" : "")
        }
        className={`h-8 w-96 rounded-md border bg-panel px-2.5 text-[13px] text-ink outline-none placeholder:text-ink-5 focus:border-accent-edge disabled:opacity-40 ${
          activeFilter ? "border-accent pr-7" : "border-edge"
        }`}
      />
      {activeFilter && (
        <button
          onClick={() => {
            // ✕ clears the filter AND the box — a lingering query one
            // Enter away from resurrecting the filter reads as broken.
            onApplyFilter(null);
            setText("");
            setResults(null);
            setOpen(false);
          }}
          title={`Stop filtering by "${activeFilter}"`}
          aria-label="Clear the view filter"
          className="absolute top-1/2 right-1.5 flex h-5 w-5 -translate-y-1/2 items-center justify-center rounded text-accent-ink hover:bg-raised"
        >
          ✕
        </button>
      )}
      {open && results && (
        <div className="absolute top-9 right-0 z-50 w-[26rem] overflow-hidden rounded-md border border-edge-strong bg-panel shadow-xl">
          <div className="border-b border-edge px-3 py-1 text-[11px] text-ink-4">
            {results.total === 0
              ? "No matches"
              : results.hits.length < results.total
                ? `Largest ${results.hits.length} of ${formatNumber(results.total)} matches`
                : `${formatNumber(results.total)} ${results.total === 1 ? "match" : "matches"}`}
          </div>
          {results.hits.length > 0 && (
            <div className="max-h-80 overflow-y-auto py-1">
              {results.hits.map((h, i) => (
                <button
                  key={h.id}
                  data-i={i}
                  className={`flex w-full items-center gap-2 px-3 py-1 text-left text-xs ${
                    i === active ? "bg-raised" : ""
                  }`}
                  onMouseMove={() => setActive(i)}
                  onClick={() => choose(h)}
                  title={h.path}
                >
                  <span
                    className={`max-w-[50%] shrink-0 truncate ${
                      h.isDir ? "text-ink" : "text-ink-2"
                    }`}
                  >
                    {h.name}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-[11px] text-ink-5">
                    {parentDir(h.path)}
                  </span>
                  <span className="tnum shrink-0 text-ink-3">
                    {formatBytes(h.size)}
                  </span>
                </button>
              ))}
            </div>
          )}
          {canFilter && (
            <div className="border-t border-edge px-3 py-1 text-[11px] text-ink-5">
              ↵ filters every view&ensp;·&ensp;↑↓ then ↵ opens a match
            </div>
          )}
        </div>
      )}
    </div>
  );
}
