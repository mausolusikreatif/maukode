import { describe, expect, it } from "vitest";
import type { SessionUsage } from "../api/types";
import { contextText, shortCount, usageText } from "./format";

const usage = (over: Partial<SessionUsage> = {}): SessionUsage => ({
  input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0,
  cost_microusd: 0, unpriced_responses: 0, context_tokens: 0, ...over,
});

describe("usage formatting", () => {
  it("shortens token counts", () => {
    expect(shortCount(950)).toBe("950");
    expect(shortCount(1000)).toBe("1k");
    expect(shortCount(12_345)).toBe("12k");
    expect(shortCount(1_234)).toBe("1.2k");
    expect(shortCount(2_500_000)).toBe("2.5M");
  });

  it("shows tokens and the known cost", () => {
    expect(usageText(null)).toBe("");
    expect(usageText(usage())).toBe("");
    expect(usageText(usage({ input_tokens: 1200, output_tokens: 34, cost_microusd: 42_000 }))).toBe("1.2k token · $0.042");
    expect(usageText(usage({ input_tokens: 900_000, output_tokens: 100_000, cost_microusd: 3_456_000 }))).toBe("1M token · $3.46");
  });

  it("shows the latest request's size as the context, not the total", () => {
    expect(contextText(null)).toBe("");
    expect(contextText(usage())).toBe("");
    expect(contextText(usage({ input_tokens: 546_661, context_tokens: 30_200 }))).toBe("30k");
  });

  it("marks a partial cost and hides an unknown one", () => {
    expect(usageText(usage({ input_tokens: 10, cost_microusd: 1000, unpriced_responses: 1 }))).toBe("10 token · $0.001+");
    expect(usageText(usage({ input_tokens: 10, unpriced_responses: 2 }))).toBe("10 token");
  });
});
