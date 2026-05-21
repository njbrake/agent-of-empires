import { test, expect } from "./helpers/mockedTest";
import type { Page } from "@playwright/test";
import { clickSidebarSession } from "./helpers/sidebar";
import {
  mockTerminalApis,
  installTerminalSpies,
  readFontSize,
  seedSettings,
} from "./helpers/terminal-mocks";

// Desktop viewport: covers the Ctrl+wheel / trackpad pinch code path that
// only runs when window.innerWidth >= MOBILE_BREAKPOINT_PX. Also proves the
// settings-change → live-font-sync useEffect doesn't reopen the PTY.

test.use({ viewport: { width: 1280, height: 800 }, hasTouch: false });

test.describe("Terminal Ctrl+wheel zoom (desktop)", () => {
  async function openSession(page: Page) {
    await clickSidebarSession(page, "pinch-test");
    await page
      .locator(".xterm")
      .first()
      .waitFor({ state: "visible", timeout: 10_000 });
  }

  async function wsCount(page: Page) {
    return page.evaluate(
      () => (window as unknown as { __WS_COUNT__: number }).__WS_COUNT__,
    );
  }

  // Dispatch wheel events on .xterm with configurable ctrlKey/deltaY.
  async function fireWheel(
    page: Page,
    opts: {
      deltaY: number;
      ctrlKey: boolean;
      times?: number;
      xRatio?: number;
      yRatio?: number;
    },
  ) {
    await page.evaluate(
      ({ deltaY, ctrlKey, times, xRatio, yRatio }) => {
        const target = document.querySelector<HTMLElement>(".xterm");
        if (!target) throw new Error(".xterm not mounted");
        const rect = target.getBoundingClientRect();
        const clientX = rect.left + rect.width * (xRatio ?? 0.5);
        const clientY = rect.top + rect.height * (yRatio ?? 0.5);
        for (let i = 0; i < (times ?? 1); i++) {
          target.dispatchEvent(
            new WheelEvent("wheel", {
              bubbles: true,
              cancelable: true,
              deltaY,
              ctrlKey,
              clientX,
              clientY,
            }),
          );
        }
      },
      opts,
    );
  }

  test("Ctrl+wheel up increases desktopFontSize after debounce", async ({
    page,
  }) => {
    await installTerminalSpies(page);
    await mockTerminalApis(page);
    await page.goto("/");
    await seedSettings(page, { desktopFontSize: 14 });
    await page.reload();
    await openSession(page);

    expect(await readFontSize(page, "desktop")).toBe(14);
    const wsBefore = await wsCount(page);

    // Each event contributes -(-60)*0.05 = +3 to the accumulator, so one
    // event with deltaY=-60 should bump size by 3. Fire twice to leave no
    // doubt.
    await fireWheel(page, { deltaY: -60, ctrlKey: true, times: 2 });

    await expect
      .poll(() => readFontSize(page, "desktop"), { timeout: 2_000 })
      .toBeGreaterThan(14);
    expect(await wsCount(page)).toBe(wsBefore);
  });

  test("Ctrl+wheel down decreases desktopFontSize", async ({ page }) => {
    await installTerminalSpies(page);
    await mockTerminalApis(page);
    await page.goto("/");
    await seedSettings(page, { desktopFontSize: 14 });
    await page.reload();
    await openSession(page);

    await fireWheel(page, { deltaY: 60, ctrlKey: true, times: 2 });

    await expect
      .poll(() => readFontSize(page, "desktop"), { timeout: 2_000 })
      .toBeLessThan(14);
  });

  test("wheel without ctrlKey does not change font size (scrolls terminal instead)", async ({
    page,
  }) => {
    await installTerminalSpies(page);
    const terminal = await mockTerminalApis(page);
    await page.goto("/");
    await seedSettings(page, { desktopFontSize: 14 });
    await page.reload();
    await openSession(page);

    // Clear any writes from seeding.
    await page.evaluate(() => {
      (window as unknown as { __LS_WRITES__: string[] }).__LS_WRITES__ = [];
    });

    await fireWheel(page, {
      deltaY: -120,
      ctrlKey: false,
      times: 5,
      xRatio: 0.83,
      yRatio: 0.21,
    });

    // 500ms is longer than the 400ms debounce — if the handler leaked
    // writes through without ctrlKey, they would have landed by now.
    await page.waitForTimeout(500);
    const writes = await page.evaluate(() =>
      (window as unknown as { __LS_WRITES__: string[] }).__LS_WRITES__.filter(
        (w) => w.includes("desktopFontSize"),
      ),
    );
    expect(writes).toEqual([]);
    expect(await readFontSize(page, "desktop")).toBe(14);

    const wheelMessages = terminal.wsMessages
      .map((m) => m.toString("utf8"))
      .filter((m) => m.startsWith("\x1b[<64;") || m.startsWith("\x1b[<65;"));
    expect(wheelMessages.length).toBeGreaterThan(0);
    expect(wheelMessages.every((m) => !m.includes(";1;1M"))).toBe(true);

    const wheelCoords = wheelMessages
      .map((m) => /\x1b\[<\d+;(\d+);(\d+)[Mm]/.exec(m))
      .filter((m): m is RegExpExecArray => m !== null)
      .map(([, col, row]) => ({ col: Number(col), row: Number(row) }));
    expect(wheelCoords.length).toBeGreaterThan(0);
    expect(wheelCoords.some(({ col, row }) => col > 1 && row > 1)).toBe(true);
  });

  test("Ctrl+wheel zoom does NOT re-mount the terminal (live-sync regression guard)", async ({
    page,
  }) => {
    // Regression guard for the load-bearing change in this PR: the main
    // terminal useEffect no longer depends on the font-size setting, so
    // persisting a new font size (via pinch/wheel → update()) must NOT
    // tear down and rebuild the terminal. We can't drive this via the
    // settings UI because SettingsView fully replaces the app view (and
    // unmounts TerminalView). Instead, we tag the live .xterm before a
    // Ctrl+wheel zoom and assert the same element survives the persist.
    await installTerminalSpies(page);
    await mockTerminalApis(page);
    await page.goto("/");
    await seedSettings(page, { desktopFontSize: 14 });
    await page.reload();
    await openSession(page);

    const tag = `xterm-${Date.now()}`;
    await page.evaluate((id) => {
      const el = document.querySelector(".xterm");
      if (!el) throw new Error("no .xterm to tag");
      el.setAttribute("data-test-id", id);
    }, tag);

    await fireWheel(page, { deltaY: -60, ctrlKey: true, times: 2 });

    await expect
      .poll(() => readFontSize(page, "desktop"), { timeout: 2_000 })
      .toBeGreaterThan(14);
    // If the main effect had re-run on settings change, the tagged
    // element would have been wiped by `container.innerHTML = ""`.
    const stillThere = await page.evaluate(
      (id) => !!document.querySelector(`[data-test-id="${id}"]`),
      tag,
    );
    expect(stillThere).toBe(true);
  });
});
