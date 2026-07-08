// Single funnel for backend failures; stale-generation races stay silent.

type Listener = (message: string) => void;

let listener: Listener | null = null;

export function onUiError(l: Listener): () => void {
  listener = l;
  return () => {
    if (listener === l) listener = null;
  };
}

export function reportUiError(context: string, error: unknown): void {
  const message = `${context}: ${String(error)}`;
  console.error(message);
  listener?.(message);
}

/** Expected failure of a query that raced a scan restart. */
export function isStale(error: unknown): boolean {
  const s = String(error);
  return (
    s.includes("stale scan generation") ||
    s.includes("no scan has been started")
  );
}

export function reportUnlessStale(context: string, error: unknown): void {
  if (!isStale(error)) reportUiError(context, error);
}
