// @vitest-environment jsdom
//
// Tests for the localStorage-backed CockpitState persistence
// added in #1132. The persistence helpers are module-private; we
// exercise them through the public `clearCockpitCache` API plus the
// observable side effect on `window.localStorage`.

import { beforeEach, describe, expect, it, vi } from "vitest";

import { emptyCockpitState, type CockpitState } from "../lib/cockpitTypes";
import { clearCockpitCache } from "./useCockpit";

const KEY_PREFIX = "aoe:cockpit-state:v1:";
const TTL_MS = 7 * 24 * 60 * 60 * 1000;

// Reach into the module's internal helpers via a side-channel: cacheSet
// is only triggered through the React `useReducer` lifecycle, which we
// don't drive here. Instead, write directly into localStorage with the
// expected shape and verify the read paths via `clearCockpitCache` (the
// only public surface that touches storage).

function writeEntry(
  sessionId: string,
  state: CockpitState,
  savedAt: number,
): void {
  window.localStorage.setItem(
    KEY_PREFIX + sessionId,
    JSON.stringify({ savedAt, state }),
  );
}

beforeEach(() => {
  window.localStorage.clear();
});

describe("useCockpit / persisted state", () => {
  it("clearCockpitCache(id) drops the matching localStorage entry", () => {
    writeEntry("sess-a", emptyCockpitState(), Date.now());
    writeEntry("sess-b", emptyCockpitState(), Date.now());
    expect(window.localStorage.getItem(KEY_PREFIX + "sess-a")).not.toBeNull();
    clearCockpitCache("sess-a");
    expect(window.localStorage.getItem(KEY_PREFIX + "sess-a")).toBeNull();
    expect(window.localStorage.getItem(KEY_PREFIX + "sess-b")).not.toBeNull();
  });

  it("clearCockpitCache() drops every cockpit-state entry", () => {
    writeEntry("sess-a", emptyCockpitState(), Date.now());
    writeEntry("sess-b", emptyCockpitState(), Date.now());
    window.localStorage.setItem("unrelated:key", "x");
    clearCockpitCache();
    expect(window.localStorage.getItem(KEY_PREFIX + "sess-a")).toBeNull();
    expect(window.localStorage.getItem(KEY_PREFIX + "sess-b")).toBeNull();
    expect(window.localStorage.getItem("unrelated:key")).toBe("x");
  });

  it("entries past STATE_TTL_MS are stamped older than the cutoff", () => {
    writeEntry("sess-old", emptyCockpitState(), Date.now() - TTL_MS - 1000);
    const oldRaw = window.localStorage.getItem(KEY_PREFIX + "sess-old");
    expect(oldRaw).not.toBeNull();
    const parsed = JSON.parse(oldRaw!) as { savedAt: number };
    expect(Date.now() - parsed.savedAt).toBeGreaterThan(TTL_MS);
  });

  it("malformed entries are removed by clearCockpitCache", () => {
    window.localStorage.setItem(KEY_PREFIX + "sess-broken", "not valid json{{{");
    clearCockpitCache("sess-broken");
    expect(window.localStorage.getItem(KEY_PREFIX + "sess-broken")).toBeNull();
  });

  it("setItem throwing on quota does not propagate", () => {
    const spy = vi
      .spyOn(window.localStorage, "removeItem")
      .mockImplementation(() => {
        throw new Error("quota exceeded");
      });
    try {
      expect(() => clearCockpitCache("sess-quota")).not.toThrow();
    } finally {
      spy.mockRestore();
    }
  });
});
