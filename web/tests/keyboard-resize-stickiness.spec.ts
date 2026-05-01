import { test, expect, devices, type Page } from "@playwright/test";
import { mockTerminalApis, type MockHandle } from "./helpers/terminal-mocks";

// Regression for the SIGWINCH-on-every-soft-keyboard-cycle bug.
//
// useMobileKeyboard previously exposed only `keyboardHeight` (live), and
// TerminalView padded its viewport by that. Every time the soft keyboard
// dismissed, paddingBottom flipped back to 0, the wterm container grew,
// ResizeObserver fired, and a fresh PTY resize landed at the server.
// claude (and any non-fullscreen TUI) redrew on the SIGWINCH and stacked
// banners into tmux scrollback.
//
// The fix latches `reservedKeyboardHeight` (the largest occlusion seen)
// and pads by that. The pane stays at the keyboard-reserved size whether
// the keyboard is currently up or not, so showing/hiding it produces
// zero new resize messages after the initial latch. iOS PWA / iOS 26
// Safari shrink innerHeight with the keyboard, which would also shrink
// the App root via 100dvh; App.tsx pins the root to a measured pixel
// height so that path is also stable.

test.use({ ...devices["iPhone 13"] });

interface ResizeMsg {
  type: "resize";
  cols: number;
  rows: number;
}

function extractResizes(handle: MockHandle): ResizeMsg[] {
  const out: ResizeMsg[] = [];
  for (const msg of handle.wsMessages) {
    const s = msg.toString("utf8");
    if (!s.startsWith("{")) continue;
    try {
      const parsed = JSON.parse(s);
      if (parsed?.type === "resize") out.push(parsed);
    } catch {
      // not json
    }
  }
  return out;
}

// Override visualViewport.height (and optionally innerHeight) to simulate
// a keyboard event. Matches the helper in mobile-keyboard.spec.ts.
async function setKeyboard(page: Page, opts: { open: boolean; px?: number; pwa?: boolean }) {
  await page.evaluate(
    ({ open, px, pwa }) => {
      const vv = window.visualViewport;
      if (!vv) return;
      const fullH =
        (window as unknown as { __fullH?: number }).__fullH ??
        window.innerHeight;
      (window as unknown as { __fullH?: number }).__fullH = Math.max(
        fullH,
        window.innerHeight,
      );

      if (open) {
        const newVvH = fullH - px!;
        Object.defineProperty(vv, "height", {
          get: () => newVvH,
          configurable: true,
        });
        if (pwa) {
          Object.defineProperty(window, "innerHeight", {
            get: () => newVvH,
            configurable: true,
          });
        }
      } else {
        const proto = Object.getPrototypeOf(vv);
        const orig = Object.getOwnPropertyDescriptor(proto, "height");
        if (orig) Object.defineProperty(vv, "height", orig);
        const origInner = Object.getOwnPropertyDescriptor(
          Window.prototype,
          "innerHeight",
        );
        if (origInner) Object.defineProperty(window, "innerHeight", origInner);
      }
      vv.dispatchEvent(new Event("resize"));
    },
    { open: opts.open, px: opts.px ?? 320, pwa: opts.pwa ?? false },
  );
}

async function openSession(page: Page, handle: MockHandle) {
  // Sidebar is collapsed on mobile; open it before clicking the session row.
  const sidebarToggle = page.getByRole("button", { name: "Toggle sidebar" });
  if (await sidebarToggle.isVisible()) {
    await sidebarToggle.click();
    await page.waitForTimeout(200);
  }
  await page.locator('button:has-text("pinch-test")').nth(1).click();
  await page
    .locator('[data-term="agent"] .wterm')
    .waitFor({ state: "visible", timeout: 10_000 });
  await expect
    .poll(() => handle.wsMessages.length, { timeout: 5_000 })
    .toBeGreaterThan(0);
}

test.describe("Keyboard cycle stickiness regression", () => {
  test("Safari mode: kb show/hide after initial latch produces zero resizes", async ({
    page,
  }) => {
    const handle = await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page, handle);
    await page.waitForTimeout(1000);

    // First kb open: latches the reservation. Allowed to produce one
    // resize (or zero, if the seed already covered the occlusion).
    await setKeyboard(page, { open: true, px: 320, pwa: false });
    await page.waitForTimeout(500);

    const baselineCount = extractResizes(handle).length;

    // Now cycle: close, open, close, open, close. Each cycle should
    // produce ZERO new resize messages — the whole point of the fix.
    for (const open of [false, true, false, true, false]) {
      await setKeyboard(page, { open, px: 320, pwa: false });
      await page.waitForTimeout(300);
    }

    const afterCycles = extractResizes(handle).length;
    const delta = afterCycles - baselineCount;
    expect(
      delta,
      `kb show/hide after initial latch produced ${delta} extra resize msgs ` +
        `(expected 0). Full sequence: ${JSON.stringify(extractResizes(handle))}`,
    ).toBe(0);
  });

  test("PWA mode: innerHeight shrinks with kb but App root stays pinned, no resize", async ({
    page,
  }) => {
    const handle = await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page, handle);
    await page.waitForTimeout(1000);

    // First kb open in PWA mode (faked innerHeight shrinks alongside vv).
    // We don't use page.setViewportSize here: that changes the actual
    // browser window, which has different CSS-unit and ResizeObserver
    // behavior than the production iOS PWA case (where layout viewport
    // shrinks but the OS-level window is unchanged).
    await setKeyboard(page, { open: true, px: 320, pwa: true });
    await page.waitForTimeout(500);

    const baselineCount = extractResizes(handle).length;

    for (const open of [false, true, false, true, false]) {
      await setKeyboard(page, { open, px: 320, pwa: true });
      await page.waitForTimeout(300);
    }

    const afterCycles = extractResizes(handle).length;
    const delta = afterCycles - baselineCount;
    expect(
      delta,
      `PWA kb cycles after latch produced ${delta} extra resize msgs ` +
        `(expected 0). Full sequence: ${JSON.stringify(extractResizes(handle))}`,
    ).toBe(0);
  });

  test("App root is pinned to stableViewportHeight on mobile", async ({
    page,
  }) => {
    const handle = await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page, handle);
    await page.waitForTimeout(1000);

    const before = await page.evaluate(() => {
      const root = document.querySelector<HTMLElement>(
        "div.h-dvh.flex.flex-col",
      );
      return {
        innerHeight: window.innerHeight,
        rootInlineHeight: root?.style?.height ?? "",
      };
    });

    // The hook latches max(innerHeight, vv.height) into stableViewportHeight
    // and App.tsx applies it as inline pixel height. Without this, 100dvh
    // shrinks on iOS PWA and the terminal pane shrinks with it.
    expect(before.rootInlineHeight).toMatch(/^\d+px$/);
    expect(parseInt(before.rootInlineHeight)).toBeGreaterThanOrEqual(
      before.innerHeight - 5,
    );
  });

  test("fullscreen toggle FAB releases the reservation (one explicit resize)", async ({
    page,
  }) => {
    const handle = await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page, handle);
    await page.waitForTimeout(1000);

    // Latch a reservation so the FAB renders.
    await setKeyboard(page, { open: true, px: 320, pwa: false });
    await page.waitForTimeout(500);
    await setKeyboard(page, { open: false, pwa: false });
    await page.waitForTimeout(300);

    const fab = page.locator(
      '[data-term="agent"] >> button[aria-label="Expand terminal to fullscreen"]',
    );
    await expect(fab).toBeVisible();

    const before = extractResizes(handle).length;
    await fab.click();
    await page.waitForTimeout(500);
    const afterOn = extractResizes(handle).length;

    // Toggling fullscreen ON releases paddingBottom -> the wterm
    // container grows -> exactly one resize message.
    expect(afterOn - before).toBeGreaterThanOrEqual(1);
    expect(afterOn - before).toBeLessThanOrEqual(2);

    const fabExit = page.locator(
      '[data-term="agent"] >> button[aria-label="Exit fullscreen terminal"]',
    );
    await fabExit.click();
    await page.waitForTimeout(500);
    const afterOff = extractResizes(handle).length;

    expect(afterOff - afterOn).toBeGreaterThanOrEqual(1);
    expect(afterOff - afterOn).toBeLessThanOrEqual(2);
  });
});
