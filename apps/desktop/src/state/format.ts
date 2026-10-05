/** Last path segment: the project's name. */
export function baseName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** Compact relative time for lists ("now", "5m", "3h", "2d"). */
export function shortAgo(iso: string): string {
  const seconds = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
  if (seconds < 60) return "now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
}

/** Branch name without the `maukode/` prefix, which every agent branch shares. */
export function shortBranch(branch: string): string {
  return branch.replace(/^maukode\//, "");
}
