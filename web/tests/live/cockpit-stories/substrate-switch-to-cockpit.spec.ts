// User story: switching substrate (tmux → cockpit) via the
// SwitchSubstrateAction trigger on the terminal view.
//
// Seed a non-cockpit session, navigate to it (TerminalView renders),
// click the substrate-switch icon, confirm in the dialog, and assert
// the parent flips to CockpitView once the session-list poll picks up
// the new `cockpit_mode`.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { waitForCockpitView } from "../../helpers/cockpit";

base("substrate switch from terminal to cockpit mounts the cockpit view", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-substrate" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const target = sessions.find((s) => s.title === "story-substrate");
    if (!target) throw new Error("seeded session 'story-substrate' missing");
    const sessionId = target.id;

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);

    const trigger = page.getByRole("button", {
      name: "Switch to cockpit mode",
    });
    await expect(trigger).toBeVisible({ timeout: 10_000 });
    await trigger.click();

    await expect(
      page.getByRole("heading", { name: /Switch to cockpit mode/i }),
    ).toBeVisible({ timeout: 5_000 });
    await page.getByRole("button", { name: "Switch", exact: true }).click();

    // Session-list poll lands within a few seconds; once cockpit_mode
    // flips, App.tsx renders CockpitView and the composer mounts.
    await waitForCockpitView(page, 20_000);
  } finally {
    await serve.stop();
  }
});
