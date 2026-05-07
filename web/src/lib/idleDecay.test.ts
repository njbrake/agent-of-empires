import { describe, expect, it } from "vitest";
import { resolveIdleDecayWindowMs } from "./idleDecay";

describe("resolveIdleDecayWindowMs", () => {
  it("falls back to the dashboard default when the setting is missing", () => {
    expect(resolveIdleDecayWindowMs(null)).toBe(0);
    expect(resolveIdleDecayWindowMs({})).toBe(0);
    expect(resolveIdleDecayWindowMs({ theme: {} })).toBe(0);
  });

  it("converts minutes to milliseconds", () => {
    expect(resolveIdleDecayWindowMs({ theme: { idle_decay_minutes: 5 } })).toBe(
      5 * 60 * 1000,
    );
  });

  it("clamps negative values to zero", () => {
    expect(resolveIdleDecayWindowMs({ theme: { idle_decay_minutes: -3 } })).toBe(0);
  });
});
