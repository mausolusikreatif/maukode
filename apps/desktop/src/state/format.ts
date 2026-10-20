import type { SessionUsage } from "../api/types";

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

/** Token count in short form: 950, 12.3k, 1.2M. */
export function shortCount(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0).replace(/\.0$/, "")}k`;
  return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
}

/**
 * A session's usage so far: "12.3k token · $0.042". The cost is left out when no response had a
 * known price, and marked "+" when only some had one.
 */
export function usageText(u: SessionUsage | null): string {
  if (!u) return "";
  const tokens = u.input_tokens + u.output_tokens;
  if (tokens === 0) return "";
  let out = `${shortCount(tokens)} token`;
  if (u.cost_microusd > 0 || u.unpriced_responses === 0) {
    const usd = u.cost_microusd / 1_000_000;
    out += ` · $${usd.toFixed(usd < 1 ? 3 : 2)}`;
    if (u.unpriced_responses > 0) out += "+";
  }
  return out;
}
