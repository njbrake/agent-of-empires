// User story: on mobile, plain Enter inserts a newline, does NOT send.
//
// `decideEnterAction` in Composer.tsx returns "newline" for touch-primary
// devices on plain Enter. The textarea must accept the newline natively
// and no agent_message_chunk should be produced (the composer never
// dispatches the prompt). Mobile users tap the Send button to send.

import { test as base, expect, devices } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { waitForCockpitReady, waitForCockpitView } from "../../helpers/cockpit";

base.use({ ...devices["iPhone 13"] });

base("mobile plain Enter inserts newline and does not send", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-mobile-enter" }),
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

    const composer = page.getByRole("textbox", { name: /Send a message/i });
    // Mobile skips composer auto-focus, so click into it first.
    await composer.click();
    await composer.fill("line one");
    await composer.press("Enter");
    await composer.pressSequentially("line two");

    await expect(composer).toHaveValue("line one\nline two");
    // Default fake turn would produce this text if a prompt were sent.
    // Brief wait to let any wrongly-dispatched WS round-trip surface.
    await page.waitForTimeout(500);
    await expect(page.getByText("Hello from fake ACP agent.")).toHaveCount(0);
  } finally {
    await serve.stop();
  }
});
