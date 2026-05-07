import { test, expect, type Page } from "@playwright/test";
import { mockTerminalApis, type MockHandle } from "./helpers/terminal-mocks";
import { clickSidebarSession } from "./helpers/sidebar";

test.use({ viewport: { width: 1280, height: 800 }, hasTouch: false });

async function openSession(page: Page, handle: MockHandle) {
  await clickSidebarSession(page, "pinch-test");
  await page
    .locator(".wterm")
    .first()
    .waitFor({ state: "visible", timeout: 10_000 });
  await expect
    .poll(() => handle.wsMessages.length, { timeout: 5_000 })
    .toBeGreaterThan(0);
}

function sentText(handle: MockHandle, start: number) {
  return handle.wsMessages
    .slice(start)
    .map((msg) => msg.toString("utf8"));
}

test.describe("Terminal IME input", () => {
  test("plain printable keys still send text", async ({ page }) => {
    const handle = await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page, handle);

    const start = handle.wsMessages.length;
    await page.locator(".wterm").first().locator("textarea").focus();
    await page.keyboard.type("a");

    await expect
      .poll(() => sentText(handle, start), { timeout: 5_000 })
      .toContain("a");
  });

  test("macOS Chinese composition sends only the committed text", async ({
    page,
  }) => {
    const handle = await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page, handle);

    const start = handle.wsMessages.length;
    await page.evaluate(() => {
      const ta = document.querySelector<HTMLTextAreaElement>(".wterm textarea");
      if (!ta) throw new Error("wterm textarea not found");
      ta.focus();

      ta.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "n",
          code: "KeyN",
          bubbles: true,
          cancelable: true,
        }),
      );
      ta.dispatchEvent(
        new CompositionEvent("compositionstart", {
          data: "",
          bubbles: true,
          cancelable: true,
        }),
      );
      ta.dispatchEvent(
        new CompositionEvent("compositionupdate", {
          data: "n",
          bubbles: true,
          cancelable: true,
        }),
      );
      ta.dispatchEvent(
        new CompositionEvent("compositionend", {
          data: "你好",
          bubbles: true,
          cancelable: true,
        }),
      );
    });

    await expect
      .poll(() => sentText(handle, start), { timeout: 5_000 })
      .toContain("你好");
    expect(sentText(handle, start)).not.toContain("n");
  });
});
