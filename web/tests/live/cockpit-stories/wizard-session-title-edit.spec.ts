// User story: edit the session title on the wizard's session step.
//
// Open the wizard from the group-level New session button (preselects
// project, lands on Agent step), advance to the Session step, type a
// title, and assert the field captures it. Confirms the wizardReducer
// updates `data.title` from input changes.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { inputByLabel } from "../../helpers/cockpit";

base("wizard session step records the title", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-wizard-title" }),
  });

  try {
    await page.goto(serve.baseUrl);
    const groupHeader = page.locator('[data-testid="sidebar-group-header"]').first();
    await groupHeader.getByRole("button", { name: /New session in /i }).click();

    // Agent step renders first since project is preselected. Skip
    // through by clicking Next twice (or until SessionStep mounts).
    // SessionStep's heading is "Name your session".
    // Prefill.path lands the wizard on the Session step directly.
    await expect(
      page.getByRole("heading", { name: "Name your session", exact: true }),
    ).toBeVisible({ timeout: 15_000 });

    const titleField = inputByLabel(page, "Session title");
    await expect(titleField).toBeVisible({ timeout: 10_000 });
    await titleField.fill("my-session-title");
    await expect(titleField).toHaveValue("my-session-title");
  } finally {
    await serve.stop();
  }
});
