// User story: change the logging default level and assert it persists
// across reload.

import { test as base, expect } from "@playwright/test";
import { spawnAoeServe } from "../../helpers/aoeServe";

base("logging default-level SelectField round-trips through the UI", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
  });

  try {
    await page.goto(`${serve.baseUrl}/settings`);
    const level = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: "Default level" }) })
      .locator("select")
      .first();
    await expect(level).toBeVisible({ timeout: 10_000 });

    const options = await level
      .locator("option")
      .evaluateAll((els) => (els as HTMLOptionElement[]).map((o) => o.value));
    const current = await level.inputValue();
    const next = options.find((v) => v && v !== current);
    expect(next, "logging-level select needs at least one option distinct from current").toBeDefined();

    await level.selectOption(next!);
    await expect(level).toHaveValue(next!);

    await page.reload();
    const reloaded = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: "Default level" }) })
      .locator("select")
      .first();
    await expect(reloaded).toHaveValue(next!, { timeout: 10_000 });
  } finally {
    await serve.stop();
  }
});
