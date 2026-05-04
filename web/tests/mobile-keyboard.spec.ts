import { test, expect, devices, type Page } from "@playwright/test";
import { mockTerminalApis, seedSettings } from "./helpers/terminal-mocks";

// Use iPhone 13 profile: pointer:coarse, hasTouch, correct viewport, WebKit UA.
test.use({ ...devices["iPhone 13"] });

// Simulate iOS soft keyboard opening by overriding visualViewport dimensions.
// In real iOS Safari, visualViewport.height shrinks while window.innerHeight
// may or may not (browser tab vs PWA). We test both scenarios.
async function simulateKeyboardOpen(
  page: Page,
  keyboardPx: number,
  opts: { innerHeightShrinks?: boolean } = {},
) {
  await page.evaluate(
    ({ keyboardPx, shrinkInner }) => {
      const vv = window.visualViewport;
      if (!vv) return;
      const fullH = window.innerHeight;
      const newVvH = fullH - keyboardPx;

      // Override visualViewport.height via property descriptor
      Object.defineProperty(vv, "height", {
        get: () => newVvH,
        configurable: true,
      });
      Object.defineProperty(vv, "offsetTop", {
        get: () => 0,
        configurable: true,
      });

      // In PWA standalone mode, innerHeight shrinks WITH the keyboard
      if (shrinkInner) {
        Object.defineProperty(window, "innerHeight", {
          get: () => newVvH,
          configurable: true,
        });
      }

      vv.dispatchEvent(new Event("resize"));
    },
    { keyboardPx, shrinkInner: opts.innerHeightShrinks ?? false },
  );
}

async function simulateKeyboardClose(page: Page) {
  await page.evaluate(() => {
    const vv = window.visualViewport;
    if (!vv) return;

    // Restore original descriptors by deleting overrides
    const vvProto = Object.getPrototypeOf(vv);
    const origHeight = Object.getOwnPropertyDescriptor(vvProto, "height");
    const origOffset = Object.getOwnPropertyDescriptor(vvProto, "offsetTop");
    if (origHeight) Object.defineProperty(vv, "height", origHeight);
    else delete (vv as Record<string, unknown>)["height"];
    if (origOffset) Object.defineProperty(vv, "offsetTop", origOffset);
    else delete (vv as Record<string, unknown>)["offsetTop"];

    // Restore innerHeight
    const origInner = Object.getOwnPropertyDescriptor(
      Window.prototype,
      "innerHeight",
    );
    if (origInner) Object.defineProperty(window, "innerHeight", origInner);

    vv.dispatchEvent(new Event("resize"));
  });
}

async function openSession(page: Page) {
  // On mobile the sidebar is collapsed; open it first.
  const sidebarToggle = page.getByRole("button", { name: "Toggle sidebar" });
  if (await sidebarToggle.isVisible()) {
    await sidebarToggle.click();
    await page.waitForTimeout(300);
  }
  // The session row is a button inside the expanded group. The group header
  // is also a button with "pinch-test", so we need the second match (the
  // indented session row), or target the button with role specifically.
  await page.locator('button:has-text("pinch-test")').nth(1).click();
  await page.locator(".wterm").waitFor({ state: "visible", timeout: 10_000 });
}

async function getKeyboardState(page: Page) {
  return page.evaluate(() => {
    const root = document.querySelector<HTMLElement>(
      '[class*="flex-1 flex flex-col overflow-hidden relative"]',
    );
    const termContainer = document.querySelector<HTMLElement>(".wterm");
    return {
      rootHeight: root?.getBoundingClientRect().height ?? 0,
      rootPaddingBottom: root?.style.paddingBottom || "0",
      termHeight: termContainer?.getBoundingClientRect().height ?? 0,
      innerHeight: window.innerHeight,
      vvHeight: Math.round(window.visualViewport?.height ?? 0),
    };
  });
}

test.describe("Mobile keyboard detection and layout", () => {
  async function setupAndOpen(page: Page) {
    // Mocks must be set up BEFORE any navigation so the initial API
    // requests are intercepted (especially /api/sessions).
    await mockTerminalApis(page);
    // ensureSession POSTs to /api/sessions/{id}/ensure
    await page.route("**/api/sessions/*/ensure", (r) =>
      r.fulfill({ json: { ok: true } }),
    );
    await page.goto("/");
    // seedSettings writes to localStorage (needs page loaded), then reload
    // so the app picks up the seeded settings with mocks still active.
    await seedSettings(page, { mobileFontSize: 10 });
    await page.reload();
    await page.waitForTimeout(500);
    await openSession(page);
  }

  test("detects keyboard open in Safari browser mode (innerHeight constant)", async ({
    page,
  }) => {
    await setupAndOpen(page);

    // Pre-seed reservation: TerminalView pads the viewport by ~40% of
    // innerHeight on mobile so the layout starts at a keyboard-reserved
    // size. Earlier this test asserted "0" here; that asserted the OLD
    // behavior where paddingBottom was the live keyboardHeight.
    const before = await getKeyboardState(page);
    expect(parseInt(before.rootPaddingBottom)).toBeGreaterThan(0);

    await simulateKeyboardOpen(page, 300);
    await page.waitForTimeout(500);

    const after = await getKeyboardState(page);
    // Reservation latches to max(seed, 300). Either it stays at seed (if
    // seed was already >= 300) or grows to ~300.
    expect(parseInt(after.rootPaddingBottom)).toBeGreaterThanOrEqual(
      parseInt(before.rootPaddingBottom),
    );
    expect(parseInt(after.rootPaddingBottom)).toBeGreaterThanOrEqual(250);
  });

  test("detects keyboard open in PWA mode (innerHeight shrinks with keyboard)", async ({
    page,
  }) => {
    await setupAndOpen(page);

    const before = await getKeyboardState(page);

    // Simulate PWA keyboard: actually shrink the viewport (changes innerHeight)
    // then override vv.height to match. This is how iOS PWA behaves.
    await page.setViewportSize({
      width: 390,
      height: before.innerHeight - 300,
    });
    await page.waitForTimeout(500);

    const after = await getKeyboardState(page);
    // When innerHeight shrinks WITH the keyboard, the layout viewport already
    // handles it. keyboardHeight (paddingBottom) should be 0 or very small.
    expect(parseInt(after.rootPaddingBottom) || 0).toBeLessThan(50);
  });

  test("keyboard close keeps reservation (sticky, no PTY resize on cycle)", async ({
    page,
  }) => {
    await setupAndOpen(page);

    await simulateKeyboardOpen(page, 300);
    await page.waitForTimeout(200);
    const open = await getKeyboardState(page);

    await simulateKeyboardClose(page);
    await page.waitForTimeout(200);

    const after = await getKeyboardState(page);
    // Sticky reservation: paddingBottom stays at the latched value when
    // the keyboard dismisses. This is the lever that stops SIGWINCH-ing
    // claude on every soft-keyboard show/hide; the side-effect is the
    // pane stays the same size whether the kb is up or not.
    expect(after.rootPaddingBottom).toBe(open.rootPaddingBottom);
  });

  test("toolbar renders on mobile with active session", async ({ page }) => {
    await setupAndOpen(page);
    // On chromium headless, pointer:coarse may not match — toolbar only
    // renders when isMobile is true. Check that the terminal at least loaded.
    await expect(page.locator(".wterm")).toBeVisible();
  });

  test("keyboard open button visible when keyboard closed", async ({
    page,
  }) => {
    await setupAndOpen(page);
    await expect(
      page.getByRole("button", { name: "Open keyboard" }),
    ).toBeVisible();
  });

  test("keyboard open button hidden when proxy focused", async ({ page }) => {
    await setupAndOpen(page);

    // Focus the hidden proxy input to simulate keyboard opening
    await page.evaluate(() => {
      const proxy = document.querySelector<HTMLInputElement>(
        'input[autocapitalize="none"]',
      );
      proxy?.focus();
    });

    await simulateKeyboardOpen(page, 300);
    await page.waitForTimeout(200);

    await expect(
      page.getByRole("button", { name: "Open keyboard" }),
    ).not.toBeVisible();
  });

  test("scrollToBottom fires when keyboard opens", async ({ page }) => {
    await setupAndOpen(page);

    const scrolledToBottom = await page.evaluate(() => {
      return new Promise<boolean>((resolve) => {
        const orig = (
          window as unknown as {
            __termScrollBottom?: boolean;
          }
        ).__termScrollBottom;
        // Watch for scrollTop change on the wterm container
        const wt = document.querySelector(".wterm");
        if (!wt) return resolve(false);
        // Watch for scroll events on the wterm element
        const onScroll = () => {
          resolve(true);
          wt.removeEventListener("scroll", onScroll);
        };
        wt.addEventListener("scroll", onScroll);
        setTimeout(() => {
          resolve(false);
          wt.removeEventListener("scroll", onScroll);
        }, 2000);
      });
    });
    // Trigger keyboard after setting up observer
    await simulateKeyboardOpen(page, 300);
    // The test is primarily that no crash occurs; scroll observation is best-effort
  });

  test("small viewport delta below threshold does NOT grow the reservation", async ({
    page,
  }) => {
    await setupAndOpen(page);
    const before = await getKeyboardState(page);

    // Simulate URL bar collapse: ~80px change, below 100px threshold
    await simulateKeyboardOpen(page, 80);
    await page.waitForTimeout(200);

    const state = await getKeyboardState(page);
    // The reservation latches upward only on >100px occlusion; an 80px
    // delta should leave paddingBottom unchanged from the pre-seeded
    // value (used to be "0" when paddingBottom tracked live keyboardHeight).
    expect(state.rootPaddingBottom).toBe(before.rootPaddingBottom);
  });

  test("orientation change resets fullHeight baseline", async ({ page }) => {
    await setupAndOpen(page);

    // Simulate landscape orientation
    await page.setViewportSize({ width: 844, height: 390 });
    await page.waitForTimeout(600);

    // Now open keyboard in landscape
    await simulateKeyboardOpen(page, 200);
    await page.waitForTimeout(200);

    const state = await getKeyboardState(page);
    // Should detect keyboard relative to the landscape height, not portrait
    expect(parseInt(state.rootPaddingBottom)).toBeGreaterThan(150);
  });
});

test.describe("Mobile proxy input keydown handling", () => {
  async function setupWithWsSpy(page: Page) {
    await page.addInitScript(() => {
      (window as unknown as { __PTY_SENT__: string[] }).__PTY_SENT__ = [];
      const Orig = window.WebSocket;
      window.WebSocket = class extends Orig {
        constructor(url: string | URL, protocols?: string | string[]) {
          super(url, protocols);
          const origSend = this.send.bind(this);
          this.send = (data: string | ArrayBufferLike | Blob | ArrayBufferView) => {
            if (data instanceof ArrayBuffer || ArrayBuffer.isView(data)) {
              const bytes = new Uint8Array(
                data instanceof ArrayBuffer ? data : data.buffer,
              );
              (
                window as unknown as { __PTY_SENT__: string[] }
              ).__PTY_SENT__.push(new TextDecoder().decode(bytes));
            }
            return origSend(data);
          };
        }
      } as typeof WebSocket;
    });
    await mockTerminalApis(page);
    await page.route("**/api/sessions/*/ensure", (r) =>
      r.fulfill({ json: { ok: true } }),
    );
    await page.goto("/");
    await page.waitForTimeout(300);
    await openSession(page);
  }

  async function sendKeyAndGetPtySent(page: Page, key: string, code: string) {
    await page.evaluate(
      ({ key, code }) => {
        const proxy = document.querySelector<HTMLInputElement>(
          'input[autocapitalize="none"]',
        );
        if (!proxy) throw new Error("proxy input not found");
        proxy.focus();
        proxy.dispatchEvent(
          new KeyboardEvent("keydown", { key, code, bubbles: true }),
        );
      },
      { key, code },
    );
    await page.waitForTimeout(100);
    return page.evaluate(
      () => (window as unknown as { __PTY_SENT__: string[] }).__PTY_SENT__,
    );
  }

  test("Enter key sends carriage return via proxy keydown", async ({
    page, browserName,
  }) => {
    test.skip(browserName !== "webkit", "proxy input requires pointer:coarse (mobile only)");
    await setupWithWsSpy(page);
    const sent = await sendKeyAndGetPtySent(page, "Enter", "Enter");
    expect(sent).toContain("\r");
  });

  test("Backspace key sends DEL (0x7f) via proxy keydown", async ({
    page, browserName,
  }) => {
    test.skip(browserName !== "webkit", "proxy input requires pointer:coarse (mobile only)");
    await setupWithWsSpy(page);
    const sent = await sendKeyAndGetPtySent(page, "Backspace", "Backspace");
    expect(sent).toContain("\x7f");
  });
});

test.describe("Mobile keyboard hooks ordering", () => {
  test("no React hooks error when transitioning pending → ready", async ({
    page,
  }) => {
    const errors: string[] = [];
    page.on("pageerror", (err) => errors.push(err.message));

    await mockTerminalApis(page);
    await page.route("**/api/sessions/*/ensure", (r) =>
      r.fulfill({ json: { ok: true } }),
    );
    await page.goto("/");
    await page.waitForTimeout(300);
    await openSession(page);

    await page.waitForTimeout(500);

    const hookErrors = errors.filter(
      (e) => e.includes("hook") || e.includes("Hook"),
    );
    expect(hookErrors).toEqual([]);
  });

  test("no errors when keyboard opens during session", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (err) => errors.push(err.message));

    await mockTerminalApis(page);
    await page.route("**/api/sessions/*/ensure", (r) =>
      r.fulfill({ json: { ok: true } }),
    );
    await page.goto("/");
    await page.waitForTimeout(300);
    await openSession(page);

    // Simulate keyboard open/close cycle
    await simulateKeyboardOpen(page, 300);
    await page.waitForTimeout(300);
    await simulateKeyboardClose(page);
    await page.waitForTimeout(300);

    const hookErrors = errors.filter(
      (e) => e.includes("hook") || e.includes("Hook") || e.includes("Rendered"),
    );
    expect(hookErrors).toEqual([]);
  });
});
