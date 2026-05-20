import { test, expect } from "./helpers/mockedTest";
import type { Page } from "@playwright/test";
import { clickSidebarSession } from "./helpers/sidebar";

// Verifies useTerminal.ts reconnects after a WS drop and that the first
// retry fires within the expected ~1s exponential-backoff window (not the
// old fixed 5s). Guards against regressions to the backoff constants.
//
// Note: we can't use installTerminalSpies here, because Playwright's
// page.routeWebSocket installs its own WebSocket proxy AFTER addInitScript
// runs, which overwrites any window.WebSocket patch. Instead we count
// connection attempts in the route handler (Node side) and assert on that.

async function mockApisExceptWs(page: Page, sessionTitle: string) {
  await page.route("**/api/login/status", (r) =>
    r.fulfill({ json: { required: false, authenticated: true } }),
  );
  await page.route("**/api/sessions", (r) => {
    if (r.request().method() === "POST") return r.fulfill({ status: 400 });
    return r.fulfill({
      json: {
        sessions: [
          {
            id: sessionTitle,
            title: sessionTitle,
            project_path: `/tmp/${sessionTitle}`,
            group_path: "/tmp",
            tool: "claude",
            status: "Running",
            yolo_mode: false,
            created_at: new Date().toISOString(),
            last_accessed_at: null,
            last_error: null,
            branch: null,
            main_repo_path: null,
            is_sandboxed: false,
            has_terminal: true,
            profile: "default",
            workspace_repos: [],
          },
        ],
        workspace_ordering: [],
      },
    });
  });
  await page.route("**/api/sessions/*/ensure", (r) =>
    r.fulfill({ json: { ok: true } }),
  );
  await page.route("**/api/sessions/*/terminal", (r) =>
    r.fulfill({ status: 200, body: "" }),
  );
  await page.route("**/api/sessions/*/diff/files", (r) =>
    r.fulfill({ json: { files: [], per_repo_bases: [], warning: null } }),
  );
  for (const path of [
    "settings",
    "themes",
    "agents",
    "profiles",
    "groups",
    "devices",
    "docker/status",
    "about",
  ]) {
    await page.route(`**/api/${path}`, (r) =>
      r.fulfill({ json: path === "docker/status" ? {} : [] }),
    );
  }
}

async function openSession(page: Page, title: string) {
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.goto("/");
  await clickSidebarSession(page, title);
  await page.locator(".xterm").first().waitFor({ state: "visible", timeout: 10_000 });
}

test.describe("Terminal WebSocket reconnection", () => {
  test("reconnects after a dropped connection", async ({ page }) => {
    const title = "reconnect-test";
    await mockApisExceptWs(page, title);

    // Side-channel WS (shell host terminal, container ws): keep them open
    // and mute so they don't affect our main-terminal reconnect observations.
    await page.routeWebSocket(
      /\/sessions\/[^/]+\/(terminal\/ws|container-ws)$/,
      (ws) => {
        ws.onMessage(() => {});
      },
    );

    let attempts = 0;
    let firstClosedAt = 0;
    let secondOpenedAt = 0;
    await page.routeWebSocket(/\/sessions\/[^/]+\/ws$/, (ws) => {
      attempts += 1;
      const attemptNum = attempts;
      ws.onMessage(() => {});
      setTimeout(() => {
        try {
          ws.send(Buffer.from("$ "));
        } catch {
          /* may be closed */
        }
      }, 30);
      if (attemptNum === 1) {
        setTimeout(() => {
          firstClosedAt = Date.now();
          try {
            ws.close();
          } catch {
            /* already closed */
          }
        }, 150);
      } else if (attemptNum === 2) {
        secondOpenedAt = Date.now();
      }
    });

    await openSession(page, title);

    // Wait for the reconnect to fire. First retry should be ~1s after the
    // drop. 5s upper bound fails fast if we regressed to the old 5s delay.
    await expect.poll(() => attempts, { timeout: 5_000 }).toBeGreaterThanOrEqual(2);

    // Guard: both timestamps must have been set. Without this check, a 0
    // firstClosedAt would make elapsed comically large and the < 3000
    // assertion would dominate with a misleading message.
    expect(firstClosedAt).toBeGreaterThan(0);
    expect(secondOpenedAt).toBeGreaterThan(0);

    // First retry is scheduled at 1s backoff. Allow 500-3000ms to absorb
    // Playwright latency while still catching a regression to the old 5s.
    const elapsed = secondOpenedAt - firstClosedAt;
    expect(elapsed).toBeGreaterThan(500);
    expect(elapsed).toBeLessThan(3_000);

    // Second connection should be stable, no further reconnects.
    await page.waitForTimeout(1_500);
    expect(attempts).toBe(2);
  });

  test("'online' event short-circuits the backoff and dials immediately", async ({
    page,
  }) => {
    // After the first retry fires (~1s), the second backoff is ~2s. We
    // close attempt #2 mid-flight, then dispatch a window 'online' event
    // during the 4s backoff window before attempt #3 would normally fire.
    // With the #1009 fix this triggers manualReconnect → connect()
    // immediately; without it, the listener never reconnects on a CLOSED
    // socket.
    const title = "online-test";
    await mockApisExceptWs(page, title);

    await page.routeWebSocket(
      /\/sessions\/[^/]+\/(terminal\/ws|container-ws)$/,
      (ws) => {
        ws.onMessage(() => {});
      },
    );

    let attempts = 0;
    const closeTimes: number[] = [];
    const openTimes: number[] = [];
    await page.routeWebSocket(/\/sessions\/[^/]+\/ws$/, (ws) => {
      attempts += 1;
      const attemptNum = attempts;
      openTimes.push(Date.now());
      ws.onMessage(() => {});
      // Drop attempts 1 and 2 quickly to drive the retry path; let
      // attempts 3+ stay open so the test can see the third connect
      // arrive promptly after the 'online' kick.
      if (attemptNum <= 2) {
        setTimeout(() => {
          closeTimes.push(Date.now());
          try {
            ws.close();
          } catch {
            /* already closed */
          }
        }, 50);
      }
    });

    await openSession(page, title);

    // Wait for two retry cycles to land (attempts 1 + 2 closed).
    await expect.poll(() => closeTimes.length, { timeout: 6_000 }).toBe(2);

    // Fire 'online' while the third backoff is still pending (~4s).
    // Wait a short moment so the listener is firmly armed and the WS is
    // CLOSED, then dispatch.
    await page.waitForTimeout(200);
    const beforeOnline = Date.now();
    await page.evaluate(() => window.dispatchEvent(new Event("online")));

    // Attempt 3 should arrive well under the 4s backoff that would
    // otherwise gate it.
    await expect.poll(() => attempts, { timeout: 2_000 }).toBeGreaterThanOrEqual(3);
    const thirdOpenedAt = openTimes[2];
    expect(thirdOpenedAt).toBeDefined();
    expect(thirdOpenedAt! - beforeOnline).toBeLessThan(1_500);
  });

  test("retries more than the old max of 3", async ({ page }) => {
    // The old hardcoded MAX_RETRIES was 3. The new value is 7 with
    // exponential backoff (1s, 2s, 4s, …). We don't wait the full schedule;
    // we just verify the counter climbs past the old limit to prove the new
    // constant is in effect. Budget: ~1+2+4 = 7s for 4 total attempts.
    const title = "retry-test";
    await mockApisExceptWs(page, title);

    await page.routeWebSocket(
      /\/sessions\/[^/]+\/(terminal\/ws|container-ws)$/,
      (ws) => {
        ws.onMessage(() => {});
      },
    );

    let attempts = 0;
    await page.routeWebSocket(/\/sessions\/[^/]+\/ws$/, (ws) => {
      attempts += 1;
      ws.onMessage(() => {});
      setTimeout(() => {
        try {
          ws.close();
        } catch {
          /* already closed */
        }
      }, 30);
    });

    await openSession(page, title);

    await expect
      .poll(() => attempts, { timeout: 15_000, intervals: [100, 250] })
      .toBeGreaterThanOrEqual(4);
  });
});
