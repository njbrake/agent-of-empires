// User story: delete a profile from the ProfileSelector.
//
// Create a profile, select it (so Delete renders since the selected
// profile is not the active/default), accept the confirm() prompt,
// and assert the profile is gone from the select options.

import { test as base, expect } from "@playwright/test";
import { spawnAoeServe } from "../../helpers/aoeServe";

base("delete a profile via ProfileSelector", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
  });

  try {
    page.on("dialog", (d) => void d.accept());

    await page.goto(`${serve.baseUrl}/settings`);
    const select = page.locator("select").first();
    await expect(select).toBeVisible({ timeout: 10_000 });

    await page.getByRole("button", { name: "+ New" }).click();
    await page.getByPlaceholder("Profile name").fill("delete-me");
    await page.getByRole("button", { name: "Create", exact: true }).click();
    await expect(
      select.locator("option", { hasText: "delete-me" }),
    ).toHaveCount(1, { timeout: 5_000 });

    await select.selectOption("delete-me");
    // Use title="Delete profile" to disambiguate from any other Delete
    // affordance and ensure we click the ProfileSelector Delete button.
    await page.getByTitle("Delete profile").click();

    await expect(
      select.locator("option", { hasText: "delete-me" }),
    ).toHaveCount(0, { timeout: 5_000 });
  } finally {
    await serve.stop();
  }
});
