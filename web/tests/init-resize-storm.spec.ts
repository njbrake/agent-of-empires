import { test, expect, type Page } from "@playwright/test";
import { mockTerminalApis, type MockHandle } from "./helpers/terminal-mocks";

// Regression for #807. useTerminal.ts used to read term.cols/term.rows
// inside ws.onopen, which yields wterm's hardcoded 80x24 default before
// ResizeObserver has measured the container. The result was an init-time
// resize storm: client sent 80x24 -> server resized PTY -> SIGWINCH ->
// regular-screen TUI (opencode/Claude) redrew -> previous frame stacked
// into tmux scrollback as garbled output. Fix gates the ws.onopen
// resize sends on a lastMeasuredRef populated by wterm's onResize and
// also seeds the WTerm constructor with a pre-measured size, so the
// 80x24 default never leaves the client.

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

    // Generous settle window: ResizeObserver, font swap, panel mounts,
    // and the longer initial debounce inside useTerminal all need to
    // resolve before we sample the resize message stream.
    await page.waitForTimeout(1000);

    const resizes = extractResizes(handle);
    expect(resizes.length).toBeGreaterThan(0);

    const default80x24 = resizes.filter((r) => r.cols === 80 && r.rows === 24);
    expect(
      default80x24,
      `Saw ${default80x24.length} resize msgs at wterm's 80x24 default. ` +
        `useTerminal must seed cols/rows into the WTerm constructor and ` +
        `gate ws.onopen sends on lastMeasuredRef so the default never ` +
        `reaches the server. Full sequence: ` +
        JSON.stringify(resizes),
    ).toHaveLength(0);
  });
});
