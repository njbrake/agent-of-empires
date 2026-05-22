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
    // (preselected from prefill / loadLastUsedTool fallback).
    await expect(
      page.getByRole("heading", { name: /Review & Launch/i }),
    ).toBeVisible({ timeout: 10_000 });
    await page.getByRole("button", { name: /Launch session/i }).click();

    await expect(
      page.getByRole("heading", { name: /Review & Launch/i }),
    ).toBeHidden({ timeout: 20_000 });

    await page.reload();

    // saveLastUsedTool persists `claude` on submit success. The key is
    // `aoe-cockpit-last-tool` (LAST_USED_TOOL_KEY in SessionWizard.tsx).
    const stored = await page.evaluate(() =>
      localStorage.getItem("aoe-cockpit-last-tool"),
    );
    expect(stored).toBe("claude");
  } finally {
    await serve.stop();
  }
});
