// User story: launch a session via the wizard, pressing Cmd/Ctrl+Enter
// on the Review step.
//
// Opens the wizard from the group-level New session button so the
// project path is preselected. Walks through Session → Agent (picks
// claude) → Review, then presses the chord. /api/sessions returns 201
// and a new sidebar row appears.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";

const MOD = process.platform === "darwin" ? "Meta" : "Control";

base("Cmd/Ctrl+Enter on the Review step creates the session", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-wizard-launch-seed" }),
  });

  try {
    await page.goto(serve.baseUrl);
    const groupHeader = page.locator('[data-testid="sidebar-group-header"]').first();
    await groupHeader.getByRole("button", { name: /New session in /i }).click();

    // Wizard opens at the Session step (prefill.path advances to step 1).
    await expect(
      page.getByRole("heading", { name: "Name your session", exact: true }),
    ).toBeVisible({ timeout: 10_000 });

    // Give the session a deterministic title so the new row is easy to
    // pick out of the sidebar afterwards.
    const titleField = page
      .locator("div")
      .filter({ has: page.locator("label", { hasText: "Session title" }) })
      .locator("input")
      .first();
    await titleField.fill("story-launched");

    await page.getByRole("button", { name: "Next" }).click();

    // Agent step: pick claude.
    await expect(
      page.getByRole("heading", { name: /^Choose an agent$|^Agent$/i }),
    ).toBeVisible({ timeout: 10_000 });
    await page.getByRole("button", { name: "claude", exact: true }).click();
    await page.getByRole("button", { name: "Next" }).click();

    // Review step.
    await expect(
      page.getByRole("heading", { name: /Review & Launch/i }),
    ).toBeVisible({ timeout: 10_000 });

    const before = await listSessions(serve.baseUrl);
    await page.keyboard.press(`${MOD}+Enter`);

    await expect
      .poll(
        async () => (await listSessions(serve.baseUrl)).length,
        { timeout: 20_000 },
      )
      .toBeGreaterThan(before.length);

    const rows = page.locator('[data-testid="sidebar-session-row"]');
    await expect(rows.filter({ hasText: "story-launched" })).toHaveCount(1, {
      timeout: 15_000,
    });
  } finally {
    await serve.stop();
  }
});
