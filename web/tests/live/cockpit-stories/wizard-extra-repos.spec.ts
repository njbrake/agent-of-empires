// User story: add an extra repo path to a wizard session via the
// ExtraReposPicker free-text input.
//
// After selecting a primary project, the ExtraReposPicker mounts under
// the selection. Free-text path + Add (or Enter) appends to
// data.extraRepoPaths; a chip renders with a Remove button.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";

base("wizard ExtraReposPicker accepts a free-text path", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-extra-repos" }),
  });

  try {
    await page.goto(serve.baseUrl);
    // Use the global New session trigger so ProjectStep is the first
    // step (group-level prefill would skip past it).
    await page.getByRole("button", { name: "New session", exact: true }).first().click();
    await expect(
      page.getByRole("heading", { name: "Project folder", exact: true }),
    ).toBeVisible({ timeout: 10_000 });

    // Pick a recent project so `data.path` is set; ExtraReposPicker
    // renders below the recent rows on the same Project step.
    const recentRow = page
      .locator("button")
      .filter({ hasText: "project" })
      .first();
    await recentRow.click();

    const extra = page.getByPlaceholder("/path/to/another/repo");
    await expect(extra).toBeVisible({ timeout: 10_000 });
    await extra.fill("/extra/repo-path");
    await page.getByRole("button", { name: "Add", exact: true }).click();

    await expect(
      page.getByRole("button", { name: "Remove repo-path" }),
    ).toBeVisible({ timeout: 5_000 });
  } finally {
    await serve.stop();
  }
});
