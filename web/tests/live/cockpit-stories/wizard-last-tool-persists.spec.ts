// User story: the wizard remembers the last-picked agent across
// reloads.
//
// SessionWizard persists `data.tool` to localStorage key
// "aoe-wizard-last-tool" on every submit. Reopen the wizard later and
// the agent picker pre-selects that tool, so users iterating on a
// project don't have to repeat the choice.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";

base("wizard remembers the last-picked agent after reload", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-wizard-last-tool-seed" }),
  });

  try {
    await page.goto(serve.baseUrl);
    const groupHeader = page.locator('[data-testid="sidebar-group-header"]').first();
    await groupHeader.getByRole("button", { name: /New session in /i }).click();

    // Group click opens the wizard on Review with `data.tool = claude`
    // (preselected from prefill / loadLastUsedTool fallback). Jump
    // back to the Agent step and pick a non-default tool so the
    // persistence test actually proves persistence: if save/restore
    // is broken, the wizard would fall back to "claude" on reload
    // and a claude → claude round-trip would pass falsely.
    await expect(
      page.getByRole("heading", { name: /Review & Launch/i }),
    ).toBeVisible({ timeout: 10_000 });
    await page.getByRole("button", { name: /^Agent / }).first().click();
    await expect(
      page.getByRole("heading", { name: /Choose an agent/i }),
    ).toBeVisible({ timeout: 10_000 });
    await page.getByRole("button", { name: /^codex/i }).first().click();
    await page.getByRole("button", { name: /Next/i }).click();
    await expect(
      page.getByRole("heading", { name: /Review & Launch/i }),
    ).toBeVisible({ timeout: 10_000 });
    await page.getByRole("button", { name: /Launch session/i }).click();

    await expect(
      page.getByRole("heading", { name: /Review & Launch/i }),
    ).toBeHidden({ timeout: 20_000 });

    await page.reload();

    // saveLastUsedTool persists the chosen tool on submit success.
    // The key is `aoe-cockpit-last-tool` (LAST_USED_TOOL_KEY in
    // SessionWizard.tsx). localStorage alone is an implementation
    // detail; the user story is that reopening the wizard preselects
    // the tool in the rendered DOM. Reopen the wizard and assert
    // against the Review step's Agent row.
    const stored = await page.evaluate(() =>
      localStorage.getItem("aoe-cockpit-last-tool"),
    );
    expect(stored).toBe("codex");

    const reopenedHeader = page
      .locator('[data-testid="sidebar-group-header"]')
      .first();
    await expect(reopenedHeader).toBeVisible({ timeout: 10_000 });
    await reopenedHeader
      .getByRole("button", { name: /New session in /i })
      .click();
    await expect(
      page.getByRole("heading", { name: /Review & Launch/i }),
    ).toBeVisible({ timeout: 10_000 });
    const agentRow = page.getByRole("button", { name: /^Agent / });
    await expect(agentRow).toContainText("codex", { timeout: 5_000 });
  } finally {
    await serve.stop();
  }
});
