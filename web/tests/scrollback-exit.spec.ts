import { test, expect, type Page } from "@playwright/test";
import {
  mockTerminalApis,
  installTerminalSpies,
  seedSettings,
  type MockHandle,
} from "./helpers/terminal-mocks";

// Regression guard for the "Back to live" button. Context: the mobile
// scroll-wrap bug was that tmux's default `WheelUpPane` binding uses
// `copy-mode -e`, which auto-exits copy-mode when the user scrolls
// down past the bottom. That snap-to-live discards the user's scroll
// position, which felt like "looping". The server-side fix is the
// binding override in src/tmux/utils.rs (enters copy-mode without -e
// for aoe_* sessions). The client-side counterpart: once tmux won't
// auto-exit, users need an explicit way out. That's what these tests
// verify — the button appears after a wheel-up and sends Escape.
test.use({ viewport: { width: 1280, height: 800 }, hasTouch: false });

const WHEEL_UP_SEQ = "\x1b[<64;1;1M";
const ESC = "\x1b";

function countSeq(handle: MockHandle, seq: string): number {
  const needle = Buffer.from(seq);
  let count = 0;
  for (const msg of handle.wsMessages) {
    let idx = 0;
    while ((idx = msg.indexOf(needle, idx)) !== -1) {
      count++;
      idx += needle.length;
    }
  }
  return count;
}

async function openSession(page: Page, handle: MockHandle) {
  await page.locator('button:has-text("pinch-test")').nth(1).click();
  await page.locator(".wterm").first().waitFor({ state: "visible" });
  await expect
    .poll(() => handle.wsMessages.length, { timeout: 5_000 })
    .toBeGreaterThan(0);
}

async function fireWheel(page: Page, deltaY: number, times: number) {
  await page.evaluate(
    ({ deltaY, times }) => {
      const target = document.querySelector<HTMLElement>(".wterm");
      if (!target) throw new Error(".wterm not mounted");
      for (let i = 0; i < times; i++) {
        target.dispatchEvent(
          new WheelEvent("wheel", {
            bubbles: true,
            cancelable: true,
            deltaY,
          }),
        );
      }
    },
    { deltaY, times },
  );
}

test.describe("Scrollback exit button", () => {
  test("button appears after wheel-up and disappears when clicked", async ({
    page,
  }) => {
    await installTerminalSpies(page);
    const handle = await mockTerminalApis(page);
    await page.goto("/");
    await seedSettings(page, { desktopFontSize: 14 });
    await page.reload();
    await openSession(page, handle);

    // No button yet — nobody has scrolled.
    await expect(page.getByRole("button", { name: "Back to live" })).toHaveCount(
      0,
    );

    // Scroll up → button should appear.
    await fireWheel(page, -120, 3);
    await expect
      .poll(() => countSeq(handle, WHEEL_UP_SEQ), { timeout: 2_000 })
      .toBeGreaterThan(0);
    const btn = page.getByRole("button", { name: "Back to live" });
    await expect(btn).toBeVisible();

    // Snapshot wsMessages length so we can tell Escape was sent.
    const before = handle.wsMessages.length;
    await btn.click();

    // Button gone after click.
    await expect(btn).toHaveCount(0);

    // Escape (0x1b) should have been sent to the server.
    await expect
      .poll(() => handle.wsMessages.length, { timeout: 2_000 })
      .toBeGreaterThan(before);
    // At least one of the newly-sent messages is just ESC.
    const newMsgs = handle.wsMessages.slice(before);
    const sawEsc = newMsgs.some((m) => m.includes(Buffer.from(ESC)));
    expect(sawEsc).toBe(true);
  });

  test("button does not appear on wheel-down alone", async ({ page }) => {
    await installTerminalSpies(page);
    const handle = await mockTerminalApis(page);
    await page.goto("/");
    await seedSettings(page, { desktopFontSize: 14 });
    await page.reload();
    await openSession(page, handle);

    await fireWheel(page, 120, 3);
    // Wait a tick for any async state updates.
    await page.waitForTimeout(200);

    await expect(page.getByRole("button", { name: "Back to live" })).toHaveCount(
      0,
    );
  });
});
