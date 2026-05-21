// User story: change the tmux mouse-support setting and assert it
// persists across reload.

import { test as base, expect } from "@playwright/test";
import { spawnAoeServe } from "../../helpers/aoeServe";

base("tmux mouse SelectField round-trips through the UI", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
  });

  try {
    await page.goto(`${serve.baseUrl}/settings`);
    const mouse = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: "Mouse support" }) })
      .locator("select")
      .first();
    await expect(mouse).toBeVisible({ timeout: 10_000 });

    await mouse.selectOption("disabled");
    await expect(mouse).toHaveValue("disabled");

    await page.reload();
    const reloaded = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: "Mouse support" }) })
      .locator("select")
      .first();
    await expect(reloaded).toHaveValue("disabled", { timeout: 10_000 });
  } finally {
    await serve.stop();
  }
});
