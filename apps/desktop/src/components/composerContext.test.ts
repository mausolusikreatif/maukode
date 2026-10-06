import { describe, expect, it } from "vitest";
import {
  buildAutocompleteItems,
  chipIcon,
  estimateTokens,
  parseRepoMap,
  type ContextChip,
  type ParsedCodebase,
} from "./composerContext";

describe("chipIcon", () => {
  it("returns distinct icons for each context kind", () => {
    expect(chipIcon("file")).toBe("📄");
    expect(chipIcon("folder")).toBe("📁");
    expect(chipIcon("diff")).toBe("⎇");
    expect(chipIcon("symbol")).toBe("◈");
  });
});

describe("estimateTokens", () => {
  it("returns 0 tokens for empty prompt and no chips", () => {
    expect(estimateTokens("", [])).toEqual({ chars: 0, tokens: 0 });
  });

  it("calculates tokens correctly for text alone", () => {
    // 20 characters: ceil(20 / 4) = 5 tokens
    const text = "12345678901234567890";
    expect(estimateTokens(text, [])).toEqual({ chars: 20, tokens: 5 });
  });

  it("includes attached context chips in token estimate", () => {
    const chips: ContextChip[] = [
      { id: "1", kind: "file", value: "src/auth.rs", label: "@src/auth.rs" },
      { id: "2", kind: "diff", value: "git-diff", label: "@git-diff" },
    ];
    // chipsText is "@src/auth.rs @git-diff" (length = 22)
    // combined with "Fix bug" -> "@src/auth.rs @git-diff\n\nFix bug" (length = 31)
    const result = estimateTokens("Fix bug", chips);
    expect(result.chars).toBe(31);
    expect(result.tokens).toBe(Math.ceil(31 / 4));
  });
});

describe("parseRepoMap", () => {
  it("parses file headers, symbol signatures, and second-pass lines", () => {
    const mockMap = `
crates/maukode-core/src/auth.rs:
  │ pub fn authenticate_user(token: &str) -> Result<User>
  │ pub struct UserSession
crates/maukode-server/src/routes.rs: (fn init_routes, struct AppState)
`;
    const parsed = parseRepoMap(mockMap);

    expect(parsed.files).toContain("crates/maukode-core/src/auth.rs");
    expect(parsed.files).toContain("crates/maukode-server/src/routes.rs");

    expect(parsed.folders).toContain("crates");
    expect(parsed.folders).toContain("crates/maukode-core");
    expect(parsed.folders).toContain("crates/maukode-core/src");

    const authSymbols = parsed.symbols.filter((s) => s.file === "crates/maukode-core/src/auth.rs");
    expect(authSymbols.some((s) => s.name === "authenticate_user")).toBe(true);
    expect(authSymbols.some((s) => s.name === "UserSession")).toBe(true);

    const routeSymbols = parsed.symbols.filter((s) => s.file === "crates/maukode-server/src/routes.rs");
    expect(routeSymbols.some((s) => s.name === "fn init_routes")).toBe(true);
    expect(routeSymbols.some((s) => s.name === "struct AppState")).toBe(true);
  });

  it("handles empty or whitespace maps gracefully", () => {
    const parsed = parseRepoMap("");
    expect(parsed.files).toEqual([]);
    expect(parsed.folders).toEqual([]);
    expect(parsed.symbols).toEqual([]);
  });
});

describe("buildAutocompleteItems", () => {
  const codebase: ParsedCodebase = {
    files: ["src/components/Composer.tsx", "src/styles.css", "README.md"],
    folders: ["src", "src/components"],
    symbols: [
      { name: "Composer", file: "src/components/Composer.tsx", signature: "export function Composer()" },
      { name: "useApp", file: "src/state/store.ts", signature: "export const useApp" },
    ],
  };

  it("presents trigger actions for empty query in 'all' mode", () => {
    const items = buildAutocompleteItems({
      query: "",
      mode: "all",
      codebase,
      changedFiles: [{ path: "README.md", status: "modified" }],
      diffCount: 1,
    });

    const labels = items.map((i) => i.label);
    expect(labels).toContain("@file");
    expect(labels).toContain("@folder");
    expect(labels).toContain("@git-diff");
    expect(labels).toContain("@symbol");
    expect(labels).toContain("README.md");
  });

  it("filters suggestions by file query", () => {
    const items = buildAutocompleteItems({
      query: "comp",
      mode: "all",
      codebase,
      changedFiles: [],
      diffCount: 0,
    });

    const labels = items.map((i) => i.label);
    expect(labels).toContain("src/components/Composer.tsx");
    expect(labels).toContain("src/components/");
  });

  it("prioritizes changed files from session review", () => {
    const items = buildAutocompleteItems({
      query: "read",
      mode: "file",
      codebase,
      changedFiles: [{ path: "README.md", status: "modified" }],
      diffCount: 1,
    });

    expect(items.length).toBeGreaterThan(0);
    expect(items[0].label).toBe("README.md");
    expect(items[0].description).toContain("Changed file (modified)");
  });

  it("matches git-diff when typing diff", () => {
    const items = buildAutocompleteItems({
      query: "diff",
      mode: "all",
      codebase,
      changedFiles: [],
      diffCount: 2,
    });

    const diffItem = items.find((i) => i.value === "git-diff");
    expect(diffItem).toBeDefined();
    expect(diffItem?.description).toContain("2 changed files");
  });
});
