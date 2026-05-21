// User story: switch the cockpit's current mode via the ModePicker.
//
// ModePicker (Composer.tsx) renders a chip showing the active mode
// and opens a menu on click; selecting an entry POSTs /cockpit/mode
// and the fake-ACP emits current_mode_changed, which the cockpit
// reducer applies to flip the chip label.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { waitForCockpitReady, waitForCockpitView } from "../../helpers/cockpit";

base("ModePicker switches the cockpit mode", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-mode-picker" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const sessionId = sessions[0]!.id;
    await fetch(`${serve.baseUrl}/api/sessions/${sessionId}/cockpit/enable`, {
      method: "POST",
    });
    await waitForCockpitReady(serve.baseUrl, sessionId);

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);
    await waitForCockpitView(page);

    // ModePicker's trigger shows the current mode label. Default
    // legacy mode is "Default".
    const trigger = page
      .locator("button")
      .filter({ has: page.locator(":scope > span", { hasText: /^(Default|Plan|Accept|Bypass)$/ }) })
      .first();
    await expect(trigger).toBeVisible({ timeout: 10_000 });
    await trigger.click();

    const planMenuItem = page
      .locator('[role="menu"]')
      .getByText(/^Plan$/i)
      .first();
    await expect(planMenuItem).toBeVisible({ timeout: 5_000 });
    await planMenuItem.click();

    // The fake-ACP emits current_mode_changed on session/setMode; the
    // reducer applies that and the trigger label flips.
    await expect(trigger).toContainText(/Plan/i, { timeout: 10_000 });
  } finally {
    await serve.stop();
  }
});
