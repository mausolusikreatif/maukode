import { describe, expect, it } from "vitest";
import { filterCommands, type PaletteCommand } from "./palette";

const cmd = (id: string, label: string, hint?: string): PaletteCommand => ({ id, label, hint, run: () => undefined });

describe("command palette filtering", () => {
  const commands = [
    cmd("new", "Sesi baru"),
    cmd("settings", "Buka pengaturan"),
    cmd("s1", "Buka sesi: Perbaiki login", "maukode"),
    cmd("s2", "Buka sesi: Tulis dokumentasi", "web"),
  ];

  it("keeps every command for an empty query", () => {
    expect(filterCommands(commands, "  ").map((c) => c.id)).toEqual(["new", "settings", "s1", "s2"]);
  });

  it("needs every word and ranks label prefixes first", () => {
    expect(filterCommands(commands, "sesi").map((c) => c.id)).toEqual(["new", "s1", "s2"]);
    expect(filterCommands(commands, "buka login").map((c) => c.id)).toEqual(["s1"]);
    expect(filterCommands(commands, "web").map((c) => c.id)).toEqual(["s2"]);
    expect(filterCommands(commands, "xyz")).toEqual([]);
  });
});
