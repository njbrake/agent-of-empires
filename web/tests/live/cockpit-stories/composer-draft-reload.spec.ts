// User story: composer draft persists across a full page reload.
//
// The Composer mirrors the textarea into localStorage at
// `cockpit:draft:<sessionId>` with a 250ms debounce. After a reload
// the CockpitView's mount effect seeds the composer from the same key
// so the user does not lose their in-progress prompt.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { waitForCockpitReady, waitForCockpitView } from "../../helpers/cockpit";

base("composer draft survives a full reload", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-draft-reload" }),
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
    await composer.fill("unsent draft text");
    // Flush the 250ms debounce before reload so localStorage is written.
    await page.waitForTimeout(400);

    await page.reload();
    await waitForCockpitView(page);

    await expect(
      page.getByRole("textbox", { name: /Send a message/i }),
    ).toHaveValue("unsent draft text", { timeout: 10_000 });
  } finally {
    await serve.stop();
  }
});
