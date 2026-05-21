// User story: send a message via Enter on desktop.
//
// Drives the cockpit composer textarea, types a prompt, presses Enter,
// and asserts the streamed agent response renders into the chat area.
// The default fake-ACP turn emits a single agent_message_chunk with
// "Hello from fake ACP agent." then stops.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { waitForCockpitView , enableCockpitAndWait } from "../../helpers/cockpit";

base("send message via Enter renders agent response", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-send-enter" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const sessionId = sessions[0]!.id;

    await enableCockpitAndWait(serve.baseUrl, sessionId);

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);
    await waitForCockpitView(page);

    const composer = page.getByRole("textbox", { name: /Send a message/i });
    await composer.fill("hello agent");
    await composer.press("Enter");

    await expect(page.getByText("Hello from fake ACP agent.")).toBeVisible({
      timeout: 10_000,
    });
    await expect(composer).toHaveValue("");
  } finally {
    await serve.stop();
  }
});
