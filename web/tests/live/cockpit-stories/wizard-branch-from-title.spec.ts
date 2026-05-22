// User story: setting the session title with the branch field blank
// auto-derives the branch on the Review step.
//
// `getReviewSummary` falls back: branch = worktreeBranch || title ||
// "Auto-generated". The Review EditableRow renders summary.branch as
// its display value when the user hasn't typed a branch.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";

base("wizard derives the branch from the title on the Review step", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-branch-autogen-seed" }),
  });

  try {
    await page.goto(serve.baseUrl);
    const groupHeader = page.locator('[data-testid="sidebar-group-header"]').first();
    await groupHeader.getByRole("button", { name: /New session in /i }).click();

    await expect(
      page.getByRole("heading", { name: "Name your session", exact: true }),
    ).toBeVisible({ timeout: 10_000 });

    const titleField = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: "Session title" }) })
      .locator("input")
      .first();
    await titleField.fill("autogen-branch-here");
    await page.getByRole("button", { name: "Next" }).click();

    await expect(
      page.getByRole("heading", { name: /Which AI agent/i }),
    ).toBeVisible({ timeout: 10_000 });
    await page.getByRole("button", { name: "claude", exact: true }).click();
    await page.getByRole("button", { name: "Next" }).click();

    await expect(
      page.getByRole("heading", { name: /Review & Launch/i }),
    ).toBeVisible({ timeout: 10_000 });

    // Multiple occurrences are expected (title row + branch row);
    // verify at least one branch-shaped occurrence is visible.
    await expect(
      page.getByText("autogen-branch-here").first(),
    ).toBeVisible({ timeout: 10_000 });
    const occurrences = await page
      .getByText("autogen-branch-here")
      .count();
    expect(occurrences).toBeGreaterThanOrEqual(2);
  } finally {
    await serve.stop();
  }
});
