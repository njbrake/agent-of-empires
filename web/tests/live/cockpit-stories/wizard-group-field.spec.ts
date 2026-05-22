// User story: enter a Group value in the wizard's session step.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";

base("wizard session step records Group", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-wizard-group" }),
  });

  try {
    await page.goto(serve.baseUrl);
    const groupHeader = page.locator('[data-testid="sidebar-group-header"]').first();
    await groupHeader.getByRole("button", { name: /New session in /i }).click();

    // Prefill.path lands the wizard on the Session step directly.
    await expect(
      page.getByRole("heading", { name: "Name your session", exact: true }),
    ).toBeVisible({ timeout: 15_000 });

    const groupField = page.getByPlaceholder(
      "Optional, for organizing related sessions",
    );
    await expect(groupField).toBeVisible({ timeout: 10_000 });
    await groupField.fill("my-group");
    await expect(groupField).toHaveValue("my-group");
  } finally {
    await serve.stop();
  }
});
