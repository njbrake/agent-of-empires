// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  classifyAuthError,
  installFetchErrorToasts,
  resetTokenExpired,
  TOKEN_EXPIRED_EVENT,
} from "./fetchInterceptor";

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

describe("classifyAuthError", () => {
  it("returns null for non-401 responses", async () => {
    expect(await classifyAuthError(jsonResponse(200, { ok: true }))).toBeNull();
    expect(await classifyAuthError(jsonResponse(403, { error: "x" }))).toBeNull();
    expect(await classifyAuthError(jsonResponse(500, { error: "x" }))).toBeNull();
  });

  // Regression: when the server returns 401 with `error: "login_required"`
  // (token valid, passphrase session missing), the client must NOT treat
  // this as a token rejection. Without this distinction the user pastes
  // a fresh token, the server responds login_required, and the SPA loops
  // them back to the token-entry page with "Invalid token" forever.
  it("classifies 401 login_required as login_required", async () => {
    const res = jsonResponse(401, {
      error: "login_required",
      message: "Passphrase login required",
    });
    expect(await classifyAuthError(res)).toBe("login_required");
  });

  it("classifies 401 unauthorized as unauthorized", async () => {
    const res = jsonResponse(401, {
      error: "unauthorized",
      message: "Invalid or missing auth token",
    });
    expect(await classifyAuthError(res)).toBe("unauthorized");
  });

  it("falls back to unauthorized on non-JSON 401 body", async () => {
    const res = new Response("not json", { status: 401 });
    expect(await classifyAuthError(res)).toBe("unauthorized");
  });

  it("falls back to unauthorized on JSON 401 without an error field", async () => {
    const res = jsonResponse(401, { message: "no error key" });
    expect(await classifyAuthError(res)).toBe("unauthorized");
  });

  // The classifier clones before reading; the original body must remain
  // readable so downstream handlers (fetchJson, etc.) can still parse it.
  it("leaves the original response body readable", async () => {
    const res = jsonResponse(401, { error: "login_required" });
    await classifyAuthError(res);
    const body = await res.json();
    expect(body).toEqual({ error: "login_required" });
  });
});

// Regression for #1163: when the auth token is rejected (401
// `unauthorized`), the interceptor must also POST `/api/logout` to
// drop the still-valid passphrase session cookie. Without that, the
// SPA prompts only for the token and silently waves the user back in
// on the surviving 24-hour session, bypassing the second factor.
describe("token expiry clears server session", () => {
  // Use unique URL prefixes per test so the dedup state inside the
  // interceptor doesn't make a single test flaky if run twice.
  let originalFetch: typeof window.fetch;

  beforeEach(() => {
    originalFetch = window.fetch;
    // Force-uninstall any prior interceptor so we can re-install with a
    // fresh inner fetch under our control.
    (window as unknown as { __aoeFetchPatched?: boolean }).__aoeFetchPatched =
      false;
    resetTokenExpired();
    localStorage.clear();
  });

  afterEach(() => {
    window.fetch = originalFetch;
    (window as unknown as { __aoeFetchPatched?: boolean }).__aoeFetchPatched =
      false;
    resetTokenExpired();
    localStorage.clear();
  });

  it("posts /api/logout before dispatching TOKEN_EXPIRED_EVENT on 401 unauthorized", async () => {
    const calls: { url: string; method: string }[] = [];

    // Mock the underlying fetch BEFORE installing the interceptor so
    // the interceptor wraps our mock instead of the jsdom default.
    const mockFetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url =
        typeof input === "string"
          ? input
          : input instanceof URL
            ? input.toString()
            : input.url;
      const method = init?.method ?? "GET";
      calls.push({ url, method });

      if (url.endsWith("/api/sessions") && method === "GET") {
        return jsonResponse(401, {
          error: "unauthorized",
          message: "Invalid or missing auth token",
        });
      }
      if (url.endsWith("/api/logout") && method === "POST") {
        return jsonResponse(200, { ok: true });
      }
      return jsonResponse(404, { error: "not_found" });
    });
    window.fetch = mockFetch as unknown as typeof window.fetch;

    installFetchErrorToasts();

    let dispatched = false;
    const onExpired = () => {
      dispatched = true;
    };
    window.addEventListener(TOKEN_EXPIRED_EVENT, onExpired);

    try {
      // Trigger the 401 path through the patched fetch.
      await window.fetch("/api/sessions");

      // Give the fire-and-forget logout promise a tick to land.
      await new Promise((r) => setTimeout(r, 0));

      const logoutCall = calls.find(
        (c) => c.url.endsWith("/api/logout") && c.method === "POST",
      );
      expect(logoutCall, "expected /api/logout POST to be issued").toBeDefined();

      expect(dispatched, "TOKEN_EXPIRED_EVENT must still fire").toBe(true);
    } finally {
      window.removeEventListener(TOKEN_EXPIRED_EVENT, onExpired);
    }
  });

  it("does not post /api/logout for login_required 401 (token still valid)", async () => {
    const calls: { url: string; method: string }[] = [];

    const mockFetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url =
        typeof input === "string"
          ? input
          : input instanceof URL
            ? input.toString()
            : input.url;
      const method = init?.method ?? "GET";
      calls.push({ url, method });

      if (url.endsWith("/api/sessions") && method === "GET") {
        return jsonResponse(401, {
          error: "login_required",
          message: "Passphrase login required",
        });
      }
      return jsonResponse(404, { error: "not_found" });
    });
    window.fetch = mockFetch as unknown as typeof window.fetch;

    installFetchErrorToasts();

    await window.fetch("/api/sessions");
    await new Promise((r) => setTimeout(r, 0));

    const logoutCall = calls.find(
      (c) => c.url.endsWith("/api/logout") && c.method === "POST",
    );
    expect(
      logoutCall,
      "login_required must not trigger logout (token is still valid)",
    ).toBeUndefined();
  });
});
