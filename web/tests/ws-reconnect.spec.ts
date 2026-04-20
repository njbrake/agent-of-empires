import { test, expect, type Page } from "@playwright/test";

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
      json: [
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
        },
      ],
    });
  });
  await page.route("**/api/sessions/*/ensure", (r) =>
    r.fulfill({ json: { ok: true } }),
  );
  await page.route("**/api/sessions/*/terminal", (r) =>
    r.fulfill({ status: 200, body: "" }),
  );
  await page.route("**/api/sessions/*/diff/files", (r) =>
    r.fulfill({ json: { files: [] } }),
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
  // The sidebar renders a repo-group header and a nested session row with
  // the same text. Click the nested (last) one to open the session.
  await page.locator("button").filter({ hasText: title }).last().click();
  await page.locator(".wterm").first().waitFor({ state: "visible", timeout: 10_000 });
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
