const BYTE_UNITS = ["KB", "MB", "GB", "TB", "PB"];

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  let v = n;
  let i = -1;
  do {
    v /= 1024;
    i++;
  } while (v >= 1024 && i < BYTE_UNITS.length - 1);
  const digits = v >= 100 ? 0 : v >= 10 ? 1 : 2;
  return `${v.toFixed(digits)} ${BYTE_UNITS[i]}`;
}

const numberFormat = new Intl.NumberFormat();

export function formatNumber(n: number): string {
  return numberFormat.format(n);
}

const dateFormat = new Intl.DateTimeFormat(undefined, {
  year: "numeric",
  month: "short",
  day: "numeric",
});

export function formatDate(unixSecs: number): string {
  if (unixSecs === 0) return "—";
  return dateFormat.format(new Date(unixSecs * 1000));
}

export function formatPercent(fraction: number): string {
  return `${(fraction * 100).toFixed(1)}%`;
}

export function formatElapsed(ms: number): string {
  return ms < 10_000 ? `${(ms / 1000).toFixed(1)}s` : `${Math.round(ms / 1000)}s`;
}
