import { describe, expect, it } from "vitest";
import { SseParser } from "./sse";

describe("SseParser", () => {
  it("parses id and data across arbitrary chunk boundaries", () => {
    const p = new SseParser();
    expect(p.push('id: 1\nda')).toEqual([]);
    expect(p.push('ta: {"a":1}\n')).toEqual([]);
    expect(p.push("\nid: 2\ndata: x\n\n")).toEqual([
      { id: "1", event: undefined, data: '{"a":1}' },
      { id: "2", event: undefined, data: "x" },
    ]);
  });

  it("ignores keep-alive comments and handles CRLF and multi-line data", () => {
    const p = new SseParser();
    const out = p.push(": keep-alive\r\n\r\nid: 7\r\ndata: a\r\ndata: b\r\n\r\n");
    expect(out).toEqual([{ id: "7", event: undefined, data: "a\nb" }]);
  });

  it("does not emit incomplete trailing messages", () => {
    const p = new SseParser();
    expect(p.push("data: partial")).toEqual([]);
    expect(p.push("\n\n")).toEqual([{ id: undefined, event: undefined, data: "partial" }]);
  });
});
