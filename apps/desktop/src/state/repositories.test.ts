import { describe, expect, it } from "vitest";
import type { SessionListItem } from "../api/types";
import { recentRepositories, repositoryName } from "./repositories";

const session = (directory: string, project_dir: string | null = null): SessionListItem => ({
  id: directory,
  directory,
  project_dir,
  title: "Task",
  message_count: 0,
  provider: "test",
  model: "test",
  updated_at: "2026-01-01T00:00:00Z",
  worktree_branch: null,
  running: false,
  pending_permission_count: 0,
});

describe("recentRepositories", () => {
  it("keeps last-used paths first and uses the source project for worktree sessions", () => {
    expect(
      recentRepositories(["/code/last/"], [
        session("/tmp/worktree", "/code/project"),
        session("/code/last"),
      ]),
    ).toEqual(["/code/last", "/code/project"]);
  });

  it("removes blanks and limits the list", () => {
    const paths = Array.from({ length: 10 }, (_, index) => `/code/${index}`);
    expect(recentRepositories(["  ", ...paths], [])).toEqual(paths.slice(0, 8));
  });
});

describe("repositoryName", () => {
  it("handles Unix and Windows paths", () => {
    expect(repositoryName("/code/maukode/")).toBe("maukode");
    expect(repositoryName("C:\\code\\maukode")).toBe("maukode");
  });
});
