// Contract test for the `fetchAbout` wrapper and the `ServerAbout`
// interface it shapes. Both halves of the new `build_flavor` discriminator
// (`"debug"` | `"release"`) are exercised so the runtime branch the topbar
// reads (`serverAbout?.build_flavor === "debug"`) is locked in here at the
// API-client layer instead of only in App.tsx. Part of #1055.
//
// `fetchAbout` is otherwise only exercised by Playwright (the App boots and
// reads `/api/about` at startup); when Playwright is gated off (Vitest-only
// CI lanes, local dev) this test keeps the api-client surface for the badge
// covered.
//
// The bigger picture: every `ServerAbout` discriminator (`auth_mode`,
// `cockpit_queue_drain_mode`, `build_flavor`) needs to ride through the
// real `fetchAbout` -> `fetchJson` path so source maps register the
// interface body as hit, not just the function call.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { fetchAbout, isDebugBuild, type ServerAbout } from "./api";

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function makeAbout(overrides: Partial<ServerAbout> = {}): ServerAbout {
  return {
    version: "1.2.3",
    auth_required: false,
    passphrase_enabled: false,
    auth_mode: "none",
    read_only: false,
    behind_tunnel: false,
    profile: "default",
    cockpit_master_enabled: false,
    cockpit_show_tool_durations: true,
    cockpit_queue_drain_mode: "combined",
    cockpit_max_concurrent_resumes: 4,
    cockpit_force_end_turn_threshold_secs: 30,
    cockpit_replay_events: 0,
    build_flavor: "release",
    ...overrides,
  };
}

const fetchSpy = vi.fn<typeof fetch>();

beforeEach(() => {
  fetchSpy.mockReset();
  vi.stubGlobal("fetch", fetchSpy);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("fetchAbout", () => {
  it("returns the parsed ServerAbout payload on 200", async () => {
    const payload = makeAbout({ build_flavor: "debug" });
    fetchSpy.mockResolvedValueOnce(jsonResponse(payload));

    const about = await fetchAbout();
    expect(about).not.toBeNull();
    // Drive the same `build_flavor === "debug"` discriminator the topbar
    // uses (App.tsx -> `isDevBuild={serverAbout?.build_flavor === "debug"}`)
    // so the interface field is exercised through both branches.
    expect(about?.build_flavor).toBe("debug");
    expect(about?.build_flavor === "debug").toBe(true);
  });

  it("surfaces the release flavor unchanged", async () => {
    fetchSpy.mockResolvedValueOnce(jsonResponse(makeAbout()));

    const about = await fetchAbout();
    expect(about?.build_flavor).toBe("release");
    expect(about?.build_flavor === "debug").toBe(false);
  });

  it("returns null on non-2xx", async () => {
    fetchSpy.mockResolvedValueOnce(new Response("", { status: 500 }));
    expect(await fetchAbout()).toBeNull();
  });

  it("returns null on network failure", async () => {
    fetchSpy.mockRejectedValueOnce(new Error("offline"));
    expect(await fetchAbout()).toBeNull();
  });

  it("hits the `/api/about` endpoint", async () => {
    fetchSpy.mockResolvedValueOnce(jsonResponse(makeAbout()));
    await fetchAbout();
    expect(fetchSpy).toHaveBeenCalledWith("/api/about", undefined);
  });
});

describe("isDebugBuild", () => {
  it("returns true for a debug-flavored payload", () => {
    expect(isDebugBuild(makeAbout({ build_flavor: "debug" }))).toBe(true);
  });

  it("returns false for a release-flavored payload", () => {
    expect(isDebugBuild(makeAbout({ build_flavor: "release" }))).toBe(false);
  });

  it("returns false when the about payload is null", () => {
    expect(isDebugBuild(null)).toBe(false);
  });

  it("returns false when the about payload is undefined", () => {
    expect(isDebugBuild(undefined)).toBe(false);
  });
});
