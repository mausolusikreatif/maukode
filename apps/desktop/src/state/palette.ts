/** A command of the command palette. */
export interface PaletteCommand {
  id: string;
  label: string;
  /** Shown dimmed after the label (a shortcut or a project). */
  hint?: string;
  run: () => void;
}

/**
 * Commands matching `query`, best first: every word of the query must appear in the label or
 * hint; labels starting with the query come first, then labels containing it as a word.
 */
export function filterCommands(commands: PaletteCommand[], query: string): PaletteCommand[] {
  const q = query.trim().toLowerCase();
  if (!q) return commands;
  const words = q.split(/\s+/);
  const rank = (c: PaletteCommand) => {
    const label = c.label.toLowerCase();
    if (label.startsWith(q)) return 0;
    if (label.split(/\s+/).some((w) => w.startsWith(words[0]))) return 1;
    return 2;
  };
  return commands
    .filter((c) => {
      const text = `${c.label} ${c.hint ?? ""}`.toLowerCase();
      return words.every((w) => text.includes(w));
    })
    .map((c, i) => ({ c, i, r: rank(c) }))
    .sort((a, b) => a.r - b.r || a.i - b.i)
    .map(({ c }) => c);
}
