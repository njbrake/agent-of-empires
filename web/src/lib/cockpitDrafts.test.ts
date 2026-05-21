// @vitest-environment jsdom
//
// Storage + pub/sub contract for cockpit composer drafts. The "unsent
// draft" dot in the sidebar relies on the listener filter for cheap
// per-session updates; if the filter logic drifts, every keystroke
// re-renders every sidebar entry.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  clearDraft,
  getDraft,
  hasDraft,
  setDraft,
  subscribeDrafts,
  sweepOrphanDrafts,
} from "./cockpitDrafts";

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

describe("getDraft / setDraft", () => {
  it("returns empty string when no draft is persisted", () => {
    expect(getDraft("s-1")).toBe("");
  });

  it("round-trips a written draft", () => {
    setDraft("s-1", "hello world");
    expect(getDraft("s-1")).toBe("hello world");
  });

  it("scopes drafts per session id", () => {
    setDraft("s-1", "one");
    setDraft("s-2", "two");
    expect(getDraft("s-1")).toBe("one");
    expect(getDraft("s-2")).toBe("two");
  });

  it("empty text removes the key entirely", () => {
    setDraft("s-1", "filled");
    setDraft("s-1", "");
    expect(getDraft("s-1")).toBe("");
    expect(localStorage.getItem("cockpit:draft:s-1")).toBeNull();
  });

  it("returns empty string when localStorage.getItem throws", () => {
    const spy = vi
      .spyOn(Storage.prototype, "getItem")
      .mockImplementation(() => {
        throw new Error("blocked");
      });
    expect(getDraft("s-1")).toBe("");
    spy.mockRestore();
  });

  it("setDraft swallows localStorage write errors", () => {
    const spy = vi
      .spyOn(Storage.prototype, "setItem")
      .mockImplementation(() => {
        throw new Error("quota");
      });
    expect(() => setDraft("s-1", "x")).not.toThrow();
    spy.mockRestore();
  });
});

describe("hasDraft", () => {
  it("returns false for an empty session", () => {
    expect(hasDraft("s-1")).toBe(false);
  });

  it("returns true once a non-empty draft is written", () => {
    setDraft("s-1", "x");
    expect(hasDraft("s-1")).toBe(true);
  });

  it("returns false after clearing a draft", () => {
    setDraft("s-1", "x");
    setDraft("s-1", "");
    expect(hasDraft("s-1")).toBe(false);
  });

  it("returns false when localStorage throws", () => {
    const spy = vi
      .spyOn(Storage.prototype, "getItem")
      .mockImplementation(() => {
        throw new Error("blocked");
      });
    expect(hasDraft("s-1")).toBe(false);
    spy.mockRestore();
  });
});

describe("subscribeDrafts pub/sub", () => {
  it("fires for setDraft writes on the listener's filter set", () => {
    const cb = vi.fn();
    const unsub = subscribeDrafts(cb, new Set(["s-1"]));
    setDraft("s-1", "hello");
    expect(cb).toHaveBeenCalledTimes(1);
    unsub();
  });

  it("does not fire for sessions outside the filter set", () => {
    const cb = vi.fn();
    const unsub = subscribeDrafts(cb, new Set(["s-1"]));
    setDraft("s-2", "hello");
    expect(cb).not.toHaveBeenCalled();
    unsub();
  });

  it("fires for any draft change when filter is null", () => {
    const cb = vi.fn();
    const unsub = subscribeDrafts(cb, null);
    setDraft("s-1", "a");
    setDraft("s-7", "b");
    expect(cb).toHaveBeenCalledTimes(2);
    unsub();
  });

  it("unsubscribe stops further notifications", () => {
    const cb = vi.fn();
    const unsub = subscribeDrafts(cb, null);
    unsub();
    setDraft("s-1", "x");
    expect(cb).not.toHaveBeenCalled();
  });

  it("cross-tab storage event for the matching key fires the listener", () => {
    const cb = vi.fn();
    const unsub = subscribeDrafts(cb, new Set(["s-1"]));
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: "cockpit:draft:s-1",
        newValue: "x",
      }),
    );
    expect(cb).toHaveBeenCalledTimes(1);
    unsub();
  });

  it("cross-tab storage event for an unrelated key is ignored", () => {
    const cb = vi.fn();
    const unsub = subscribeDrafts(cb, new Set(["s-1"]));
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: "some-other-key",
        newValue: "x",
      }),
    );
    expect(cb).not.toHaveBeenCalled();
    unsub();
  });

  it("storage event for a non-filtered session does not fire", () => {
    const cb = vi.fn();
    const unsub = subscribeDrafts(cb, new Set(["s-1"]));
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: "cockpit:draft:s-other",
        newValue: "x",
      }),
    );
    expect(cb).not.toHaveBeenCalled();
    unsub();
  });

  it("storage event with null key (whole-storage wipe) fires unconditionally", () => {
    const cbFiltered = vi.fn();
    const cbWildcard = vi.fn();
    const unsub1 = subscribeDrafts(cbFiltered, new Set(["s-1"]));
    const unsub2 = subscribeDrafts(cbWildcard, null);
    window.dispatchEvent(
      new StorageEvent("storage", { key: null, newValue: null }),
    );
    expect(cbFiltered).toHaveBeenCalledTimes(1);
    expect(cbWildcard).toHaveBeenCalledTimes(1);
    unsub1();
    unsub2();
  });
});

describe("clearDraft", () => {
  it("removes the persisted key", () => {
    setDraft("s-1", "x");
    clearDraft("s-1");
    expect(localStorage.getItem("cockpit:draft:s-1")).toBeNull();
    expect(hasDraft("s-1")).toBe(false);
  });

  it("notifies filtered subscribers", () => {
    setDraft("s-1", "x");
    const cb = vi.fn();
    const unsub = subscribeDrafts(cb, new Set(["s-1"]));
    clearDraft("s-1");
    expect(cb).toHaveBeenCalledTimes(1);
    unsub();
  });

  it("is a no-op when no draft existed", () => {
    expect(() => clearDraft("s-missing")).not.toThrow();
    expect(getDraft("s-missing")).toBe("");
  });
});

describe("sweepOrphanDrafts", () => {
  it("removes drafts whose session id is not in the active set", () => {
    setDraft("s-keep", "alive");
    setDraft("s-orphan-1", "gone");
    setDraft("s-orphan-2", "also gone");
    sweepOrphanDrafts(new Set(["s-keep"]));
    expect(getDraft("s-keep")).toBe("alive");
    expect(localStorage.getItem("cockpit:draft:s-orphan-1")).toBeNull();
    expect(localStorage.getItem("cockpit:draft:s-orphan-2")).toBeNull();
  });

  it("leaves non-draft keys untouched", () => {
    localStorage.setItem("aoe:other", "untouched");
    localStorage.setItem("cockpit:draft:s-orphan", "gone");
    sweepOrphanDrafts(new Set());
    expect(localStorage.getItem("aoe:other")).toBe("untouched");
    expect(localStorage.getItem("cockpit:draft:s-orphan")).toBeNull();
  });

  it("fires a single wildcard notify when keys were removed", () => {
    setDraft("s-orphan", "gone");
    const cb = vi.fn();
    const unsub = subscribeDrafts(cb, null);
    sweepOrphanDrafts(new Set());
    expect(cb).toHaveBeenCalledTimes(1);
    unsub();
  });

  it("does not notify when nothing was removed", () => {
    setDraft("s-keep", "alive");
    const cb = vi.fn();
    const unsub = subscribeDrafts(cb, null);
    sweepOrphanDrafts(new Set(["s-keep"]));
    expect(cb).not.toHaveBeenCalled();
    unsub();
  });

  it("swallows localStorage iteration errors", () => {
    setDraft("s-orphan", "gone");
    const spy = vi
      .spyOn(Storage.prototype, "key")
      .mockImplementation(() => {
        throw new Error("blocked");
      });
    expect(() => sweepOrphanDrafts(new Set())).not.toThrow();
    spy.mockRestore();
  });

  it("handles an empty active set", () => {
    setDraft("s-a", "a");
    setDraft("s-b", "b");
    sweepOrphanDrafts(new Set());
    expect(hasDraft("s-a")).toBe(false);
    expect(hasDraft("s-b")).toBe(false);
  });
});
