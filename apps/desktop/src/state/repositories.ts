import type { SessionListItem } from "../api/types";

const STORAGE_KEY = "maukode.recentRepositories";
const LIMIT = 8;

function clean(path: string): string {
  const trimmed = path.trim();
  if (trimmed === "/" || /^[A-Za-z]:[\\/]?$/.test(trimmed)) return trimmed;
  return trimmed.replace(/[\\/]+$/, "");
}

/** Combines last-used repositories with projects discovered from daemon sessions. */
export function recentRepositories(saved: string[], sessions: SessionListItem[]): string[] {
  const fromSessions = sessions.map((session) => session.project_dir ?? session.directory);
  const seen = new Set<string>();
  return [...saved, ...fromSessions]
    .map(clean)
    .filter((path) => {
      if (!path || seen.has(path)) return false;
      seen.add(path);
      return true;
    })
    .slice(0, LIMIT);
}

export function loadRecentRepositories(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]");
    return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
  } catch {
    return [];
  }
}

export function rememberRepository(path: string, current = loadRecentRepositories()): string[] {
  const recent = recentRepositories([path, ...current], []);
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(recent));
  } catch {
    // Storage may be disabled. The current daemon sessions still populate the list.
  }
  return recent;
}

export function repositoryName(path: string): string {
  const normalized = clean(path).replace(/\\/g, "/");
  return normalized.split("/").pop() || normalized;
}
