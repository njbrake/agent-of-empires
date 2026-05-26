// User story: switch the cockpit's current mode via the ModePicker.
//
// ModePicker (Composer.tsx) renders a chip showing the active mode
// and opens a menu on click; selecting an entry POSTs /cockpit/mode
// and the fake-ACP emits current_mode_update, which the cockpit
// reducer applies to flip the chip label.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { waitForCockpitView, enableCockpitAndWait } from "../../helpers/cockpit";

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
    const seeded = sessions.find((s) => s.title === "story-mode-picker");
    if (!seeded) throw new Error("seeded session 'story-mode-picker' missing");
    const sessionId = seeded.id;
    await enableCockpitAndWait(serve.baseUrl, sessionId);
    // Explicit spawn so the supervisor has an active ACP session
    // attached before setMode dispatches. Without this, /cockpit/mode
    // can race the implicit spawn from enable and fail silently.
    const spawnRes = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/spawn`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ agent: "claude" }),
      },
    );
    if (![200, 202, 409].includes(spawnRes.status)) {
      throw new Error(`cockpit spawn failed: ${spawnRes.status}`);
    }

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

    // The fake-ACP emits current_mode_update on session/set_mode; the
    // reducer applies that and the trigger label flips.
    await expect(trigger).toContainText(/Plan/i, { timeout: 10_000 });
  } finally {
    await serve.stop();
  }
});
