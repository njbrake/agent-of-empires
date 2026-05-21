// User story: toggle the sound "Enabled" switch and assert state
// persists across reload.

import { test as base, expect } from "@playwright/test";
import { spawnAoeServe } from "../../helpers/aoeServe";

base("sound enabled toggle round-trips through the UI", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
  });

  try {
    await page.goto(`${serve.baseUrl}/settings`);

    // ToggleField renders the label in a sibling div, so the role=switch
    // button's accessible name does not include "Enabled". Locate the
    // wrapper by label text, then drill into the switch.
    const toggle = page
      .locator("div.flex.items-center.justify-between")
      .filter({ hasText: "Play sounds on session status changes" })
      .locator('button[role="switch"]');
    await expect(toggle).toBeVisible({ timeout: 10_000 });

    const beforeChecked = await toggle.getAttribute("aria-checked");
    await toggle.click();
    const after = beforeChecked === "true" ? "false" : "true";
    await expect(toggle).toHaveAttribute("aria-checked", after);

    await page.reload();
    const reloaded = page
      .locator("div.flex.items-center.justify-between")
      .filter({ hasText: "Play sounds on session status changes" })
      .locator('button[role="switch"]');
    await expect(reloaded).toHaveAttribute("aria-checked", after, {
      timeout: 10_000,
    });
  } finally {
    await serve.stop();
  }
});
