import type { SessionUsage, SystemPromptInfo } from "../api/types";

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
 * The input size of the session's latest request, "30k": what the model's context holds now. Empty
 * before the first response. Unlike {@link usageText} this is not a running total.
 */
export function contextText(u: SessionUsage | null): string {
  if (!u || u.context_tokens === 0) return "";
  return shortCount(u.context_tokens);
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

/**
 * What every request carries besides the conversation: the system prompt plus the tool schemas
 * (`fixedTokens`), and the tools whose schemas take the most.
 */
export function promptSummary(info: SystemPromptInfo, top = 5) {
  const biggest = [...info.tools].sort((a, b) => b.tokens - a.tokens).slice(0, top);
  return {
    fixedTokens: info.system_prompt_tokens + info.tool_schema_tokens,
    toolCount: info.tools.length,
    biggest,
  };
}
