import { test, expect, type Page } from "@playwright/test";
import { mockTerminalApis, type MockHandle } from "./helpers/terminal-mocks";

// Regression for #830 / #831. wterm's WASM grid (@wterm/core 0.1.x) is
// fixed at 256x256 and silently clamps any larger resize. If the host-side
// PTY is resized past this cap, the agent draws into a viewport bigger
// than wterm renders: long lines wrap inside wterm but not in the agent's
// mental model, displacing every subsequent row and producing visible
// corruption. useTerminal.sendResize must clamp before sending so the
// PTY size never exceeds wterm's grid.

const WTERM_MAX_COLS = 256;
const WTERM_MAX_ROWS = 256;

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

test.describe("wterm grid cap workaround", () => {
  // 4000x2400 viewport with the dashboard's default font produces well over
  // 256 cols (and could exceed 256 rows at small fonts). Without the clamp
  // the client would forward the unclamped measurement to the server.
  test.use({ viewport: { width: 4000, height: 2400 }, hasTouch: false });

  test("never sends a resize larger than wterm's 256x256 cap", async ({
    page,
  }) => {
    const handle = await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page, handle);

    // Match the settle window used by init-resize-storm.spec so all init,
    // font-swap, and ResizeObserver activity has completed.
    await page.waitForTimeout(1000);

    const resizes = extractResizes(handle);
    expect(resizes.length).toBeGreaterThan(0);

    const oversized = resizes.filter(
      (r) => r.cols > WTERM_MAX_COLS || r.rows > WTERM_MAX_ROWS,
    );
    expect(
      oversized,
      `Saw ${oversized.length} resize msgs above wterm's ${WTERM_MAX_COLS}x${WTERM_MAX_ROWS} ` +
        `grid cap. sendResize must clamp before sending so the PTY never ` +
        `exceeds wterm's render grid. Full sequence: ` +
        JSON.stringify(resizes),
    ).toHaveLength(0);
  });
});
