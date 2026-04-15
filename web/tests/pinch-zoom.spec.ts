import { test, expect, devices, type Page } from "@playwright/test";

// Mobile pinch-to-zoom for the terminal font size.
//
// Doesn't need a real `aoe serve`: we stub the REST API and route the PTY
// WebSocket via `page.routeWebSocket`, so the xterm.js terminal mounts and
// the pinch gesture handlers in useTerminal.ts are exercised against the
// real frontend bundle. We then synthesize two-finger `TouchEvent`s on the
// `.xterm` element (Playwright's `page.touchscreen` is single-finger) and
// assert that the font size setting in localStorage updated.

test.use({ ...devices["iPhone 13"] });

test.describe("Terminal pinch zoom", () => {
  async function mockApis(page: Page) {
    await page.route("**/api/login/status", (r) =>
      r.fulfill({ json: { required: false, authenticated: true } }),
    );
    await page.route("**/api/sessions", (r) => {
      if (r.request().method() === "POST") return r.fulfill({ status: 400 });
      return r.fulfill({
        json: [
          {
            id: "pinch-test",
            title: "pinch-test",
            project_path: "/tmp/pinch-test",
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
    // PTY WebSocket: keep open, acknowledge resize messages, ignore data.
    await page.routeWebSocket(/\/sessions\/.*\/(ws|container-ws)$/, (ws) => {
      ws.onMessage(() => {
        /* absorb keystrokes / resize JSON */
      });
      // Minimal prompt so xterm shows something (helps visual debugging too).
      setTimeout(() => ws.send(Buffer.from("$ ")), 50);
    });
  }

  // Dispatches a TouchEvent with two touches relative to `.xterm`. The
  // page.touchscreen API is single-finger only, so we build Touch objects
  // and fire the event directly.
  async function fireTouches(
    page: Page,
    type: "touchstart" | "touchmove" | "touchend",
    points: { x: number; y: number }[],
  ) {
    await page.evaluate(
      ({ type, points }) => {
        const target = document.querySelector<HTMLElement>(".xterm");
        if (!target) throw new Error(".xterm not mounted");
        const rect = target.getBoundingClientRect();
        const touches = points.map((p, i) => {
          const clientX = rect.left + p.x;
          const clientY = rect.top + p.y;
          return new Touch({
            identifier: i,
            target,
            clientX,
            clientY,
            pageX: clientX,
            pageY: clientY,
            screenX: clientX,
            screenY: clientY,
            radiusX: 2,
            radiusY: 2,
            rotationAngle: 0,
            force: 1,
          });
        });
        const ev = new TouchEvent(type, {
          bubbles: true,
          cancelable: true,
          touches: type === "touchend" ? [] : touches,
          targetTouches: type === "touchend" ? [] : touches,
          changedTouches: touches,
        });
        target.dispatchEvent(ev);
      },
      { type, points },
    );
  }

  function readFontSize(page: Page) {
    return page.evaluate(() => {
      const raw = localStorage.getItem("aoe-web-settings");
      return raw ? JSON.parse(raw).mobileFontSize : null;
    });
  }

  async function setFontSize(page: Page, size: number) {
    await page.evaluate((size) => {
      localStorage.setItem(
        "aoe-web-settings",
        JSON.stringify({ mobileFontSize: size, desktopFontSize: 14 }),
      );
    }, size);
  }

  test("two-finger spread increases saved mobile font size", async ({
    page,
  }) => {
    await mockApis(page);
    await page.goto("/");
    // Seed a known starting size so the assertion is unambiguous.
    await setFontSize(page, 10);
    await page.reload();

    // Tap the session row (uses its title).
    await page
      .getByRole("button", { name: /pinch-test claude/ })
      .first()
      .click();
    await page.locator(".xterm").waitFor({ state: "visible", timeout: 10_000 });

    const before = await readFontSize(page);
    expect(before).toBe(10);

    // Pinch-out: start with fingers 80px apart, spread to ~240px. That's a
    // 3x scale factor — ratio is clamped to MAX_FONT_SIZE = 28.
    const cx = 160;
    const cy = 200;
    await fireTouches(page, "touchstart", [
      { x: cx - 40, y: cy },
      { x: cx + 40, y: cy },
    ]);
    // Walk the fingers outward over several frames to cross the 12px lock
    // deadzone and then climb steadily.
    for (let step = 1; step <= 16; step++) {
      const spread = 40 + step * 12; // 52, 64, ... 232
      await fireTouches(page, "touchmove", [
        { x: cx - spread, y: cy },
        { x: cx + spread, y: cy },
      ]);
    }
    await fireTouches(page, "touchend", []);

    // Persist happens on touchend; give React a beat to flush.
    await expect
      .poll(() => readFontSize(page), { timeout: 2_000 })
      .toBeGreaterThan(10);
  });

  test("two-finger pinch-in decreases saved mobile font size", async ({
    page,
  }) => {
    await mockApis(page);
    await page.goto("/");
    await setFontSize(page, 14);
    await page.reload();

    await page
      .getByRole("button", { name: /pinch-test claude/ })
      .first()
      .click();
    await page.locator(".xterm").waitFor({ state: "visible", timeout: 10_000 });

    const before = await readFontSize(page);
    expect(before).toBe(14);

    const cx = 160;
    const cy = 200;
    await fireTouches(page, "touchstart", [
      { x: cx - 120, y: cy },
      { x: cx + 120, y: cy },
    ]);
    for (let step = 1; step <= 16; step++) {
      const spread = 120 - step * 6; // 114 ... 24
      await fireTouches(page, "touchmove", [
        { x: cx - spread, y: cy },
        { x: cx + spread, y: cy },
      ]);
    }
    await fireTouches(page, "touchend", []);

    await expect
      .poll(() => readFontSize(page), { timeout: 2_000 })
      .toBeLessThan(14);
  });

  test("two-finger vertical pan does NOT change font size (scroll mode)", async ({
    page,
  }) => {
    await mockApis(page);
    await page.goto("/");
    await setFontSize(page, 10);
    await page.reload();

    await page
      .getByRole("button", { name: /pinch-test claude/ })
      .first()
      .click();
    await page.locator(".xterm").waitFor({ state: "visible", timeout: 10_000 });

    // Pan: both fingers move down together, distance constant — should
    // lock into scroll mode and leave the size untouched.
    const cx = 160;
    let cy = 100;
    await fireTouches(page, "touchstart", [
      { x: cx - 50, y: cy },
      { x: cx + 50, y: cy },
    ]);
    for (let step = 1; step <= 12; step++) {
      cy = 100 + step * 16;
      await fireTouches(page, "touchmove", [
        { x: cx - 50, y: cy },
        { x: cx + 50, y: cy },
      ]);
    }
    await fireTouches(page, "touchend", []);

    // Give any async write a chance; size should still be exactly 10.
    await page.waitForTimeout(300);
    expect(await readFontSize(page)).toBe(10);
  });
});
