import { test, expect, type Page } from "@playwright/test";
import { mockTerminalApis, type MockHandle } from "./helpers/terminal-mocks";

// Workaround regression for the wterm DEC ?2026 (BSU/ESU) gap. wterm
// silently ignores the synchronized-output mode toggles, so when an agent
// brackets a multi-chunk frame redraw with `\x1b[?2026h` ... `\x1b[?2026l`,
// each chunk lands in a separate JS task and triggers its own rAF render
// (half-drawn frames). useTerminal must buffer bytes between BSU and ESU
// so wterm sees one term.write per frame.

const desktop = { width: 1280, height: 800 };
test.use({ viewport: desktop, hasTouch: false });

const BSU = "\x1b[?2026h";
const ESU = "\x1b[?2026l";

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

// Read the first terminal's row content. Reads via DOM textContent so we're
// observing what the user would see in the browser, not wterm's internal
// state. Joins all term-row elements with `|` so we can match across rows.
async function readScreen(page: Page): Promise<string> {
  return await page.locator(".wterm").first().evaluate((el) => {
    const rows = el.querySelectorAll(".term-row");
    return Array.from(rows, (r) => r.textContent ?? "").join("|");
  });
}

test.describe("wterm BSU/ESU buffering workaround", () => {
  test("frame split across BSU…ESU is held until ESU and rendered atomically", async ({
    page,
  }) => {
    const handle = await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page, handle);

    // Stage a known starting state.
    handle.push(Buffer.from("\x1b[2J\x1b[Hbefore"));
    await expect
      .poll(() => readScreen(page), { timeout: 2_000 })
      .toContain("before");

    // Open a synchronized update, then push the partial frame in two
    // separate WS messages. The intermediate state ("middle1middle2"
    // overwriting "before") must NOT be visible until ESU arrives.
    handle.push(Buffer.from(BSU + "\x1b[H\x1b[2Kmiddle1"));
    // Give the page a generous chance to render if the workaround is
    // broken; if the buffering is working, this wait is wasted but safe.
    await page.waitForTimeout(80);
    let mid = await readScreen(page);
    expect(
      mid,
      `Frame should be held during BSU. Saw partial state: ${mid}`,
    ).not.toContain("middle1");
    expect(mid).toContain("before");

    handle.push(Buffer.from("middle2"));
    await page.waitForTimeout(40);
    mid = await readScreen(page);
    expect(mid, `Frame still held mid-BSU. Saw: ${mid}`).not.toContain(
      "middle1middle2",
    );
    expect(mid).toContain("before");

    // ESU should release the buffered bytes and the terminal should now
    // show the post-ESU state.
    handle.push(Buffer.from(ESU));
    await expect
      .poll(() => readScreen(page), { timeout: 2_000 })
      .toContain("middle1middle2");
  });

  test("BSU without ESU flushes after the safety timeout", async ({ page }) => {
    const handle = await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page, handle);

    handle.push(Buffer.from("\x1b[2J\x1b[Hidle"));
    await expect
      .poll(() => readScreen(page), { timeout: 2_000 })
      .toContain("idle");

    // Send BSU + content but never close it. After ~150ms the workaround
    // should force-flush so the terminal isn't permanently held.
    handle.push(Buffer.from(BSU + "\x1b[H\x1b[2Kunclosed"));

    // Just after the push, the buffer should still be holding.
    await page.waitForTimeout(40);
    expect(await readScreen(page)).not.toContain("unclosed");

    // After the safety timeout (150ms in the workaround), it flushes.
    await expect
      .poll(() => readScreen(page), { timeout: 1_500 })
      .toContain("unclosed");
  });

  test("bytes outside any BSU pass through immediately", async ({ page }) => {
    const handle = await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page, handle);

    handle.push(Buffer.from("\x1b[2J\x1b[Hhello"));
    // Without any BSU/ESU markers, this should land synchronously
    // (modulo the existing rAF) — a short poll succeeds quickly.
    await expect
      .poll(() => readScreen(page), { timeout: 1_000 })
      .toContain("hello");
  });
});
