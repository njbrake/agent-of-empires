import { test, expect, type Page } from "@playwright/test";
import { mockTerminalApis, type MockHandle } from "./helpers/terminal-mocks";

// Regression for #807. useTerminal.ts used to read term.cols/term.rows
// inside ws.onopen, which yields wterm's hardcoded 80x24 default before
// ResizeObserver has measured the container. The result was an init-time
// resize storm: client sent 80x24 -> server resized PTY -> SIGWINCH ->
// regular-screen TUI (opencode/Claude) redrew -> previous frame stacked
// into tmux scrollback as "garbled output". Fix gates the ws.onopen
// resize sends on a lastMeasuredRef populated by wterm's onResize.
//
// This test asserts that no resize message is sent at the wterm default
// size. If the gate regresses, the storm will reappear.

const desktop = { width: 1280, height: 800 };
test.use({ viewport: desktop, hasTouch: false });

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

async function openSession(page: Page, handle: MockHandle) {
  await page.locator('button:has-text("pinch-test")').nth(1).click();
  await page
    .locator(".wterm")
    .first()
    .waitFor({ state: "visible", timeout: 10_000 });
  await expect
    .poll(() => handle.wsMessages.length, { timeout: 5_000 })
    .toBeGreaterThan(0);
}

test.describe("Init resize storm regression (#807)", () => {
  test("never sends wterm's 80x24 default at session open", async ({ page }) => {
    const handle = await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page, handle);

    // Generous settle window: ResizeObserver, font swap, panel mounts.
    await page.waitForTimeout(1000);

    const resizes = extractResizes(handle);
    expect(resizes.length).toBeGreaterThan(0);

    const default80x24 = resizes.filter((r) => r.cols === 80 && r.rows === 24);
    expect(
      default80x24,
      `Saw ${default80x24.length} resize msgs at wterm's 80x24 default. ` +
        `useTerminal must gate ws.onopen sends on lastMeasuredRef so the ` +
        `default never reaches the server. Full sequence: ` +
        JSON.stringify(resizes),
    ).toHaveLength(0);
  });

  test("first resize sent is not the wterm hardcoded default", async ({
    page,
  }) => {
    const handle = await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page, handle);

    await page.waitForTimeout(1000);

    const resizes = extractResizes(handle);
    expect(resizes.length).toBeGreaterThan(0);

    // Whatever the very first resize sent is, it must reflect a real
    // measurement, not wterm's 80x24 starting point. The exact cols/rows
    // depend on viewport, panel layout, and font metrics, so we don't
    // pin a specific number; just that it isn't the default.
    const first = resizes[0]!;
    const isDefault = first.cols === 80 && first.rows === 24;
    expect(
      isDefault,
      `First resize was wterm's 80x24 default — gate on measurement regressed`,
    ).toBe(false);
  });
});
