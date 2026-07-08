// Raw query box; grammar lives in mathom-core/src/search.rs.

import { useCallback, useEffect, useRef, useState } from "react";
import { api, type SearchHit, type SearchResults } from "../lib/api";
import { reportUnlessStale } from "../lib/errors";
import { formatBytes, formatNumber } from "../lib/format";

const DEBOUNCE_MS = 150;

interface SearchBoxProps {
  generation: number;
  hideSystem: boolean;
  onSelect: (hit: SearchHit) => void;
}

function parentDir(path: string): string {
  const i = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return i > 0 ? path.slice(0, i) : path;
}

export function SearchBox({
  generation,
  hideSystem,
  onSelect,
}: SearchBoxProps) {
  const [text, setText] = useState("");
  const [results, setResults] = useState<SearchResults | null>(null);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const seqRef = useRef(0);
  const boxRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const seq = ++seqRef.current;
    if (generation === 0 || text.trim() === "") {
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
      onSelect(hit);
      setOpen(false);
    },
    [onSelect],
  );

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      if (open) {
        setOpen(false);
      } else {
        setText("");
        inputRef.current?.blur();
      }
      return;
    }
    const hits = results?.hits ?? [];
    if (!open || hits.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, hits.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === "Enter") {
      const hit = hits[active] ?? hits[0];
      if (hit) choose(hit);
    }
  };

  return (
    <div ref={boxRef} className="relative">
      <input
        ref={inputRef}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onFocus={() => {
          if (results && text.trim() !== "") setOpen(true);
        }}
        onKeyDown={onKeyDown}
        placeholder="Search — ext:mp4 >1gb"
        spellCheck={false}
        disabled={generation === 0}
        title={
          "Space-separated filters, all must match:\nname substring · ext:mp4 · >100mb"
        }
        className="h-8 w-56 rounded-md border border-zinc-800 bg-zinc-900 px-2.5 text-[13px] text-zinc-200 outline-none placeholder:text-zinc-600 focus:border-teal-700 disabled:opacity-40"
      />
      {open && results && (
        <div className="absolute top-9 right-0 z-50 w-[26rem] overflow-hidden rounded-md border border-zinc-700 bg-zinc-900 shadow-xl">
          <div className="border-b border-zinc-800 px-3 py-1 text-[11px] text-zinc-500">
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
                    i === active ? "bg-zinc-800" : ""
                  }`}
                  onMouseMove={() => setActive(i)}
                  onClick={() => choose(h)}
                  title={h.path}
                >
                  <span
                    className={`max-w-[50%] shrink-0 truncate ${
                      h.isDir ? "text-zinc-100" : "text-zinc-300"
                    }`}
                  >
                    {h.name}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-[11px] text-zinc-600">
                    {parentDir(h.path)}
                  </span>
                  <span className="tnum shrink-0 text-zinc-400">
                    {formatBytes(h.size)}
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
