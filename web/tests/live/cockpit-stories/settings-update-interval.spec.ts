// User story: change the update check interval and assert it persists
// across reload.

import { test as base, expect } from "@playwright/test";
import { spawnAoeServe } from "../../helpers/aoeServe";

base("update check-interval NumberField round-trips through the UI", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
  });

  try {
    await page.goto(`${serve.baseUrl}/settings`);
    const input = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: "Check interval (hours)" }) })
      .locator('input[type="number"]')
      .first();
    await expect(input).toBeVisible({ timeout: 10_000 });

    await input.fill("12");
    await input.press("Enter");

    await page.reload();
    const reloaded = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: "Check interval (hours)" }) })
      .locator('input[type="number"]')
      .first();
    await expect(reloaded).toHaveValue("12", { timeout: 10_000 });
  } finally {
    await serve.stop();
  }
});
