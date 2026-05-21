// User story: toggle the "Create a worktree" switch on the wizard's
// session step.
//
// useWorktree defaults to true; toggling it off hides the worktree
// branch input. Toggling back on re-mounts the input.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";

base("wizard worktree toggle hides and shows the branch input", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-wizard-worktree" }),
  });

  try {
    await page.goto(serve.baseUrl);
    const groupHeader = page.locator('[data-testid="sidebar-group-header"]').first();
    await groupHeader.getByRole("button", { name: /New session in /i }).click();

    // Advance to SessionStep.
    for (let i = 0; i < 4; i++) {
      const sessionHeading = page.getByRole("heading", { name: "Name your session", exact: true });
      if (await sessionHeading.isVisible()) break;
      const next = page.getByRole("button", { name: /^Next$/ });
      if (await next.isVisible()) await next.click();
      else break;
      await page.waitForTimeout(150);
    }

    const branchLabel = page.getByText("Branch / worktree name");
    await expect(branchLabel).toBeVisible({ timeout: 10_000 });

    // Toggle worktree off via the label/click region next to the switch.
    await page.getByText("Create a worktree").click();
    await expect(branchLabel).toBeHidden({ timeout: 5_000 });

    await page.getByText("Create a worktree").click();
    await expect(branchLabel).toBeVisible({ timeout: 5_000 });
  } finally {
    await serve.stop();
  }
});
