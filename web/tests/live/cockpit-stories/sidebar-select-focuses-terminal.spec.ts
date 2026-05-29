// User story (#1454): clicking a non-cockpit session row in the sidebar
// lands keyboard focus on that session's xterm textarea, so the user can
// type immediately without a second click. Desktop only; on a coarse
// pointer the click must NOT focus the textarea (no soft-keyboard pop).

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";

async function activeElementInXterm(page: import("@playwright/test").Page) {
  return page.evaluate(() => {
    const active = document.activeElement as HTMLElement | null;
    return Boolean(active && active.closest(".xterm"));
  });
}

base(
  "desktop: sidebar select focuses the terminal textarea",
  async ({ page }, testInfo) => {
    const serve = await spawnAoeServe({
      authMode: "none",
      workerIndex: testInfo.workerIndex,
      parallelIndex: testInfo.parallelIndex,
      seedFn: seedSessionViaAoeAdd({ title: "story-focus-term" }),
    });

    try {
      const sessions = await listSessions(serve.baseUrl);
      const seeded = sessions.find((s) => s.title === "story-focus-term");
      if (!seeded) throw new Error("seeded session 'story-focus-term' missing");

      // Start on the dashboard so the click is what navigates + focuses.
      await page.goto(serve.baseUrl);
      const row = page
        .locator('[data-testid="sidebar-session-row"]')
        .first();
      await expect(row).toBeVisible({ timeout: 10_000 });

      await row.click();
      await expect(page).toHaveURL(
        new URL(
          `/session/${encodeURIComponent(seeded.id)}`,
          serve.baseUrl,
        ).toString(),
        { timeout: 10_000 },
      );

      await expect
        .poll(() => activeElementInXterm(page), { timeout: 10_000 })
        .toBe(true);
    } finally {
      await serve.stop();
    }
  },
);

base.describe("coarse pointer", () => {
  base.use({ hasTouch: true, isMobile: true });

  base(
    "coarse pointer: sidebar select does not focus the terminal textarea",
    async ({ page }, testInfo) => {
      const serve = await spawnAoeServe({
        authMode: "none",
        workerIndex: testInfo.workerIndex,
        parallelIndex: testInfo.parallelIndex,
        seedFn: seedSessionViaAoeAdd({ title: "story-focus-term-coarse" }),
      });

      try {
        const sessions = await listSessions(serve.baseUrl);
        const seeded = sessions.find(
          (s) => s.title === "story-focus-term-coarse",
        );
        if (!seeded) {
          throw new Error("seeded session 'story-focus-term-coarse' missing");
        }

        await page.goto(serve.baseUrl);
        const row = page
          .locator('[data-testid="sidebar-session-row"]')
          .first();
        await expect(row).toBeVisible({ timeout: 10_000 });

        await row.click();
        await expect(page).toHaveURL(
          new URL(
            `/session/${encodeURIComponent(seeded.id)}`,
            serve.baseUrl,
          ).toString(),
          { timeout: 10_000 },
        );

        // Give any stray focus dispatch a beat to land, then assert focus
        // never entered the terminal (which would pop the soft keyboard).
        await page.waitForTimeout(1_000);
        expect(await activeElementInXterm(page)).toBe(false);
      } finally {
        await serve.stop();
      }
    },
  );
});
