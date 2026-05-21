// User story: change a tmux setting via the SelectField and confirm
// the new value persists across a reload.
//
// Drives the SelectField directly: locate the wrapper div by its
// label text, change the underlying select, then reload and check
// the value re-renders.

import { test as base, expect } from "@playwright/test";
import { spawnAoeServe } from "../../helpers/aoeServe";

base("tmux status_bar setting select round-trips through the UI", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
  });

  try {
    await page.goto(`${serve.baseUrl}/settings`);

    // Find the SelectField wrapper by its unique label text and pull
    // out its inline <select>. SelectField wraps <label> + optional
    // description + <select> inside one outer div.
    const statusBar = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: "Status bar" }) })
      .locator("select")
      .first();
    await expect(statusBar).toBeVisible({ timeout: 10_000 });

    await statusBar.selectOption("disabled");
    await expect(statusBar).toHaveValue("disabled");

    await page.reload();
    const reloaded = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: "Status bar" }) })
      .locator("select")
      .first();
    await expect(reloaded).toHaveValue("disabled", { timeout: 10_000 });
  } finally {
    await serve.stop();
  }
});
