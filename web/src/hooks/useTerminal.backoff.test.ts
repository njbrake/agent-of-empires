// @vitest-environment jsdom
//
// Unit tests for retryDelayMs. The function drives the WS reconnect
// backoff schedule (1s, 2s, 4s, 8s, 16s, 30s, 30s); a regression here
// would either hammer a dead server every second or stretch the first
// retry past the user-perceptible threshold.

import { describe, expect, it } from "vitest";
import { retryDelayMs } from "./useTerminal";

describe("retryDelayMs", () => {
  it("doubles each attempt up to the 30s cap", () => {
    expect(retryDelayMs(1)).toBe(1000);
    expect(retryDelayMs(2)).toBe(2000);
    expect(retryDelayMs(3)).toBe(4000);
    expect(retryDelayMs(4)).toBe(8000);
    expect(retryDelayMs(5)).toBe(16000);
  });

  it("caps at 30s for the tail of the backoff", () => {
    expect(retryDelayMs(6)).toBe(30000);
    expect(retryDelayMs(7)).toBe(30000);
    // Defense against an off-by-one: even an out-of-range attempt
    // never exceeds the cap, so the retry handler can't accidentally
    // schedule a 60s+ timeout if MAX_RETRIES creeps up.
    expect(retryDelayMs(20)).toBe(30000);
  });
});
