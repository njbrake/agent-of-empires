// User story: clicking @ or / on the composer toolbar inserts the
// trigger character into the textarea.
//
// ToolbarButton aria-labels are "Add file context (@)" and
// "Slash command (/)"; both call insertAtCaret on the textarea ref.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { waitForCockpitReady, waitForCockpitView } from "../../helpers/cockpit";

base("composer toolbar inserts @ and / into the textarea", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-composer-toolbar" }),
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
    await composer.click();

    await page.getByRole("button", { name: "Add file context (@)" }).click();
    await expect(composer).toHaveValue("@");

    await page.getByRole("button", { name: "Slash command (/)" }).click();
    await expect(composer).toHaveValue("@/");
  } finally {
    await serve.stop();
  }
});
