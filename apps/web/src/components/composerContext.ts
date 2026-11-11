export type ContextKind = "file" | "folder" | "diff" | "symbol";

export interface ContextChip {
  id: string;
  kind: ContextKind;
  value: string;
  label: string;
  detail?: string;
}

export interface AutocompleteItem {
  id: string;
  kind: "action" | ContextKind;
  value: string;
  label: string;
  description: string;
  icon: string;
}

export interface ParsedCodebase {
  files: string[];
  folders: string[];
  symbols: { name: string; file: string; signature: string }[];
}

export function chipIcon(kind: ContextKind): string {
  switch (kind) {
    case "file":
      return "📄";
    case "folder":
      return "📁";
    case "diff":
      return "⎇";
    case "symbol":
      return "◈";
  }
}

/**
 * Calculates a lightweight token and character estimate for the composer prompt
 * including any attached context badges.
 * Standard heuristic: ~4 characters per token.
 */
export function estimateTokens(
  text: string,
  chips: ContextChip[],
): { chars: number; tokens: number } {
  const chipsText = chips.map((c) => `@${c.value}`).join(" ");
  const combined = chipsText ? (text.trim() ? `${chipsText}\n\n${text}` : chipsText) : text;
  const chars = combined.length;
  const tokens = chars === 0 ? 0 : Math.max(1, Math.ceil(chars / 4));
  return { chars, tokens };
}

function extractFolders(filePath: string, folderSet: Set<string>) {
  const parts = filePath.split("/");
  for (let i = 1; i < parts.length; i++) {
    folderSet.add(parts.slice(0, i).join("/"));
  }
}

/**
 * Parses the skeleton codebase map output produced by the MauKode RepoMapGenerator.
 */
export function parseRepoMap(mapText: string): ParsedCodebase {
  const files: string[] = [];
  const folderSet = new Set<string>();
  const symbols: { name: string; file: string; signature: string }[] = [];
  let currentFile = "";

  for (const rawLine of mapText.split("\n")) {
    const line = rawLine.trim();
    if (!line) continue;

    // Check if line is a file block header: "crates/foo/bar.rs:" or "src/App.tsx:"
    if (line.endsWith(":") && !line.includes(" ")) {
      currentFile = line.slice(0, -1);
      if (!files.includes(currentFile)) {
        files.push(currentFile);
        extractFolders(currentFile, folderSet);
      }
      continue;
    }

    // Check second-pass format: "path/to/file.rs: (struct Foo, fn bar)"
    const secondPass = /^([^:\s]+):\s*\((.+)\)$/.exec(line);
    if (secondPass) {
      const f = secondPass[1];
      if (!files.includes(f)) {
        files.push(f);
        extractFolders(f, folderSet);
      }
      const symList = secondPass[2].split(",").map((s) => s.trim());
      for (const sym of symList) {
        if (sym) {
          symbols.push({ name: sym, file: f, signature: sym });
        }
      }
      continue;
    }

    // Check symbol lines under currentFile: "│ export function Foo..." or "  │ fn bar()"
    if (rawLine.includes("│") && currentFile) {
      const sig = rawLine.split("│").slice(1).join("│").trim();
      if (sig) {
        const nameMatch =
          /(?:fn|function|class|struct|enum|interface|type|const|let|var)\s+([a-zA-Z0-9_]+)/.exec(sig);
        const name = nameMatch ? nameMatch[1] : sig.split(/[\s(]/)[0];
        symbols.push({ name: name || sig, file: currentFile, signature: sig });
      }
    }
  }

  return {
    files,
    folders: Array.from(folderSet),
    symbols,
  };
}

/**
 * Builds and ranks autocomplete suggestions based on the typed mention query and filter mode.
 */
export function buildAutocompleteItems({
  query,
  mode,
  codebase,
  changedFiles,
  diffCount,
}: {
  query: string;
  mode: "all" | "file" | "folder" | "symbol";
  codebase: ParsedCodebase;
  changedFiles: { path: string; status: string }[];
  diffCount: number;
}): AutocompleteItem[] {
  const q = query.toLowerCase().trim();
  const items: AutocompleteItem[] = [];

  // 1. Primary mention triggers (when in "all" mode)
  if (mode === "all") {
    if (!q || "file".startsWith(q) || "@file".startsWith(q)) {
      items.push({
        id: "act-file",
        kind: "action",
        value: "file",
        label: "@file",
        description: "Attach or browse a workspace file",
        icon: "📄",
      });
    }
    if (!q || "folder".startsWith(q) || "@folder".startsWith(q)) {
      items.push({
        id: "act-folder",
        kind: "action",
        value: "folder",
        label: "@folder",
        description: "Reference a directory or project folder",
        icon: "📁",
      });
    }
    if (!q || "git-diff".startsWith(q) || "@git-diff".startsWith(q) || "diff".startsWith(q)) {
      items.push({
        id: "act-diff",
        kind: "diff",
        value: "git-diff",
        label: "@git-diff",
        description:
          diffCount > 0 ? `Include working diff (${diffCount} changed files)` : "Include working diff",
        icon: "⎇",
      });
    }
    if (!q || "symbol".startsWith(q) || "@symbol".startsWith(q)) {
      items.push({
        id: "act-symbol",
        kind: "action",
        value: "symbol",
        label: "@symbol",
        description: "Reference a code symbol, function, or struct",
        icon: "◈",
      });
    }
  }

  // 2. Changed files from session review (high priority)
  if (mode === "all" || mode === "file") {
    for (const f of changedFiles) {
      if (!q || f.path.toLowerCase().includes(q)) {
        items.push({
          id: `file-changed-${f.path}`,
          kind: "file",
          value: f.path,
          label: f.path,
          description: `Changed file (${f.status})`,
          icon: "📄",
        });
      }
    }

    // 3. Workspace files from codebase map
    for (const f of codebase.files) {
      if (changedFiles.some((cf) => cf.path === f)) continue;
      if (!q || f.toLowerCase().includes(q)) {
        items.push({
          id: `file-${f}`,
          kind: "file",
          value: f,
          label: f,
          description: "Workspace file",
          icon: "📄",
        });
      }
    }
  }

  // 4. Workspace folders
  if (mode === "all" || mode === "folder") {
    for (const dir of codebase.folders) {
      if (!q || dir.toLowerCase().includes(q)) {
        items.push({
          id: `folder-${dir}`,
          kind: "folder",
          value: dir,
          label: `${dir}/`,
          description: "Directory",
          icon: "📁",
        });
      }
    }
  }

  // 5. Code symbols
  if (mode === "all" || mode === "symbol") {
    for (const sym of codebase.symbols) {
      if (!q || sym.name.toLowerCase().includes(q) || sym.file.toLowerCase().includes(q)) {
        items.push({
          id: `sym-${sym.name}-${sym.file}`,
          kind: "symbol",
          value: sym.name,
          label: sym.name,
          description: `${sym.signature} (${sym.file})`,
          icon: "◈",
        });
      }
    }
  }

  // If query is specifically about diff and not in "all" mode
  if (mode !== "all" && ("git-diff".includes(q) || "diff".includes(q))) {
    items.unshift({
      id: "act-diff",
      kind: "diff",
      value: "git-diff",
      label: "@git-diff",
      description:
        diffCount > 0 ? `Include working diff (${diffCount} changed files)` : "Include working diff",
      icon: "⎇",
    });
  }

  return items.slice(0, 16);
}
