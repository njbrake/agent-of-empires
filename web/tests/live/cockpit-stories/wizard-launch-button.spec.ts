// User story: launch a session by clicking the Launch button on the
// wizard's Review step (mouse path).
//
// Mirrors wizard-launch-cmd-enter but exercises the click path that
// many users prefer over the keyboard chord.

import { test as base, expect } from "@playwright/test";
import { inputByLabel } from "../../helpers/cockpit";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";

base("Launch button on Review step creates the session", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-wizard-launch-button-seed" }),
  });

  try {
    await page.goto(serve.baseUrl);
    const groupHeader = page.locator('[data-testid="sidebar-group-header"]').first();
    await groupHeader.getByRole("button", { name: /New session in /i }).click();

    await expect(
      page.getByRole("heading", { name: "Name your session", exact: true }),
    ).toBeVisible({ timeout: 10_000 });
    const titleField = inputByLabel(page, "Session title");
    await titleField.fill("story-launched-button");
    await page.getByRole("button", { name: "Next" }).click();

    await expect(
      page.getByRole("heading", { name: /Which AI agent/i }),
    ).toBeVisible({ timeout: 10_000 });
    await page.getByRole("button", { name: "claude", exact: true }).click();
    await page.getByRole("button", { name: "Next" }).click();

    await expect(
      page.getByRole("heading", { name: /Review & Launch/i }),
    ).toBeVisible({ timeout: 10_000 });

    const before = await listSessions(serve.baseUrl);
    await page.getByRole("button", { name: /Launch session/i }).click();

    await expect
      .poll(async () => (await listSessions(serve.baseUrl)).length, {
        timeout: 20_000,
      })
      .toBeGreaterThan(before.length);

    await expect(
      page
        .locator('[data-testid="sidebar-session-row"]')
        .filter({ hasText: "story-launched-button" }),
    ).toHaveCount(1, { timeout: 15_000 });
  } finally {
    await serve.stop();
  }
});
