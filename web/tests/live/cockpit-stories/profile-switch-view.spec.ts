// User story: switching the selected profile via the ProfileSelector
// updates which profile's settings are being shown.
//
// Switching the dropdown calls onSelect, which the parent SettingsView
// uses to re-fetch settings for that profile. Profile-only settings
// for a fresh profile match the default profile's values, but the
// select value itself flips and persists the local selection on
// successive interactions.

import { test as base, expect } from "@playwright/test";
import { spawnAoeServe } from "../../helpers/aoeServe";

base("ProfileSelector switches the selected profile", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
  });

  try {
    await page.goto(`${serve.baseUrl}/settings`);
    const select = page.locator("select").first();
    await expect(select).toBeVisible({ timeout: 10_000 });
    const initialProfile = await select.inputValue();

    await page.getByRole("button", { name: "+ New" }).click();
    await page.getByPlaceholder("Profile name").fill("profile-switch-alt");
    await page.getByRole("button", { name: "Create", exact: true }).click();
    await expect(
      select.locator("option", { hasText: "profile-switch-alt" }),
    ).toHaveCount(1, { timeout: 5_000 });

    await select.selectOption("profile-switch-alt");
    await expect(select).toHaveValue("profile-switch-alt");

    await select.selectOption(initialProfile);
    await expect(select).toHaveValue(initialProfile);
  } finally {
    await serve.stop();
  }
});
