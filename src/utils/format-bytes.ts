/** Human-readable byte sizes for file and storage displays. */

/**
 * Format a byte count as `B`, `KB`, `MB`, or `GB` with one decimal place.
 * Returns `fallback` when the size is unknown.
 */
export function formatBytes(bytes: number | null | undefined, fallback = "—"): string {
  if (bytes === null || bytes === undefined) return fallback;
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}
