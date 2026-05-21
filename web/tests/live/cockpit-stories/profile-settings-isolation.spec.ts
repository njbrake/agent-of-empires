// User story: settings changed on one profile do not bleed into
// another profile.
//
// SettingsView saves via updateProfileSettings(selectedProfile, ...).
// On profile switch the panel re-fetches and re-renders against the
// new profile's stored values; profile B should start at the default
// regardless of edits made under profile A.

import { test as base, expect } from "@playwright/test";
import { spawnAoeServe } from "../../helpers/aoeServe";

base("per-profile settings stay isolated across profile switches", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
  });

  try {
    await page.goto(`${serve.baseUrl}/settings`);
    const profileSelect = page.locator("select").first();
    await expect(profileSelect).toBeVisible({ timeout: 10_000 });
    await expect(profileSelect).toHaveValue("default");

    const statusBar = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: "Status bar" }) })
      .locator("select")
      .first();
    await expect(statusBar).toBeVisible({ timeout: 10_000 });

    // Change tmux status_bar on the default profile to "disabled".
    await statusBar.selectOption("disabled");
    await expect(statusBar).toHaveValue("disabled");

    // Create profile B and switch to it.
    await page.getByRole("button", { name: "+ New" }).click();
    await page.getByPlaceholder("Profile name").fill("isolation-b");
    await page.getByRole("button", { name: "Create", exact: true }).click();
    await expect(
      profileSelect.locator("option", { hasText: "isolation-b" }),
    ).toHaveCount(1, { timeout: 5_000 });
    await profileSelect.selectOption("isolation-b");

    // Profile B starts fresh; tmux status_bar should be the unset
    // default ("auto"), not "disabled".
    const statusBarB = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: "Status bar" }) })
      .locator("select")
      .first();
    await expect(statusBarB).toHaveValue("auto", { timeout: 10_000 });

    // Switch back to default; the value we set above must still be
    // "disabled".
    await profileSelect.selectOption("default");
    const statusBarA = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: "Status bar" }) })
      .locator("select")
      .first();
    await expect(statusBarA).toHaveValue("disabled", { timeout: 10_000 });
  } finally {
    await serve.stop();
  }
});
