// @vitest-environment jsdom
//
// Tests for the per-session toast dedupe on draft persistence failure
// added for #1345.
//
// Contract (locked in debate):
// - Drafts are unsent user text; failure to persist is surfaced.
// - To avoid a toast storm (setDraft fires on every keystroke), each
//   session id toasts at most once per page lifetime — until a later
//   successful write clears its dedupe entry.
// - Two failing sessions toast independently.
// - State-cache writes stay silent (handled in useCockpit.ts); only
//   drafts trip this toast.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  __resetDraftPersistFailureNotifications,
  setDraft,
} from "./cockpitDrafts";
import { toastBus, type ToastApi } from "./toastBus";

function makeQuotaError(): DOMException {
  return new DOMException("The quota has been exceeded.", "QuotaExceededError");
}

function attachToastSpy(): ToastApi & {
  errors: string[];
  infos: string[];
} {
  const errors: string[] = [];
  const infos: string[] = [];
  const handler: ToastApi & { errors: string[]; infos: string[] } = {
    push(msg, kind) {
      if (kind === "error") errors.push(msg);
      else infos.push(msg);
    },
    error(msg) {
      errors.push(msg);
    },
    info(msg) {
      infos.push(msg);
    },
    errors,
    infos,
  };
  toastBus.handler = handler;
  return handler;
}

beforeEach(() => {
  window.localStorage.clear();
  __resetDraftPersistFailureNotifications();
});

afterEach(() => {
  vi.restoreAllMocks();
  toastBus.handler = null;
});

describe("cockpitDrafts toast dedupe (#1345)", () => {
  it("fires exactly one toast per session when writes fail repeatedly", () => {
    const spy = attachToastSpy();
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw makeQuotaError();
    });

    setDraft("sess-a", "hello");
    setDraft("sess-a", "hello world");
    setDraft("sess-a", "hello world!");

    expect(spy.errors).toHaveLength(1);
    expect(spy.errors[0]).toMatch(/storage full/i);
  });

  it("clears dedupe after a successful write; later failure re-toasts", () => {
    const spy = attachToastSpy();

    // First storm: setItem throws.
    const setItemSpy = vi.spyOn(Storage.prototype, "setItem");
    setItemSpy.mockImplementation(() => {
      throw makeQuotaError();
    });
    setDraft("sess-a", "x");
    setDraft("sess-a", "xy");
    expect(spy.errors).toHaveLength(1);

    // Storage frees up. The next write succeeds and clears the flag.
    setItemSpy.mockRestore();
    setDraft("sess-a", "xyz"); // succeeds against real localStorage
    expect(window.localStorage.getItem("cockpit:draft:sess-a")).toBe("xyz");

    // Storage fills up again. The next failure must re-toast.
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw makeQuotaError();
    });
    setDraft("sess-a", "xyzw");
    expect(spy.errors).toHaveLength(2);
  });

  it("two failing sessions each get their own toast (no cross-suppression)", () => {
    const spy = attachToastSpy();
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw makeQuotaError();
    });

    setDraft("sess-a", "text-a");
    setDraft("sess-b", "text-b");
    setDraft("sess-a", "text-a-more");
    setDraft("sess-b", "text-b-more");

    expect(spy.errors).toHaveLength(2);
  });

  it("does not toast when text is empty (removal); no draft to lose", () => {
    const spy = attachToastSpy();
    vi.spyOn(Storage.prototype, "removeItem").mockImplementation(() => {
      throw makeQuotaError();
    });

    setDraft("sess-a", "");

    // Empty-text path goes through safeRemoveItem, which swallows the
    // throw silently. There is no unsent text at risk, so no toast.
    expect(spy.errors).toHaveLength(0);
  });

  it("does not toast when the write succeeds", () => {
    const spy = attachToastSpy();
    setDraft("sess-a", "hello");
    expect(window.localStorage.getItem("cockpit:draft:sess-a")).toBe("hello");
    expect(spy.errors).toHaveLength(0);
  });
});
