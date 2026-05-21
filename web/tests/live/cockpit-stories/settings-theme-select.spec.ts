// User story: change the theme via the settings ThemeSettings select.
//
// The theme dropdown is loaded asynchronously from GET /api/themes.
// Once populated, picking a non-default option PATCHes settings and
// the choice survives a reload.

import { test as base, expect } from "@playwright/test";
import { spawnAoeServe } from "../../helpers/aoeServe";

base("theme name SelectField round-trips through the UI", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
  });

  try {
    await page.goto(`${serve.baseUrl}/settings`);

    const themeSelect = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: /^Theme$/ }) })
      .locator("select")
      .first();
    await expect(themeSelect).toBeVisible({ timeout: 10_000 });
    // Wait for the themes list to populate (initial render is empty).
    await expect
      .poll(async () => themeSelect.locator("option").count(), {
        timeout: 10_000,
      })
      .toBeGreaterThan(0);

    const optionValues = await themeSelect
      .locator("option")
      .evaluateAll((els) => (els as HTMLOptionElement[]).map((o) => o.value));
    const current = await themeSelect.inputValue();
    const next = optionValues.find((v) => v && v !== current);
    expect(next, "theme select needs at least one option distinct from current").toBeDefined();

    await themeSelect.selectOption(next!);
    await expect(themeSelect).toHaveValue(next!);

    await page.reload();
    const reloaded = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: /^Theme$/ }) })
      .locator("select")
      .first();
    await expect(reloaded).toHaveValue(next!, { timeout: 10_000 });
  } finally {
    await serve.stop();
  }
});
