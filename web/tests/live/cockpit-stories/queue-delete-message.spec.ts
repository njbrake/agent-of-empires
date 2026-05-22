// User story: drop a queued follow-up message via its X button.
//
// QueuedPromptRow renders a trailing X with title="Drop this queued
// message". Clicking it removes the prompt from the queue strip; the
// row disappears before the active turn ends.

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { waitForCockpitView , enableCockpitAndWait } from "../../helpers/cockpit";

const SCRIPT = {
  turns: [
    {
      updates: [
        {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "Working on turn 1..." },
        },
        { sessionUpdate: "wait_ms", ms: 8_000 },
      ],
      stopReason: "end_turn",
    },
  ],
};

base("delete a queued follow-up before it fires", async ({ page }, testInfo) => {
  const scriptDir = mkdtempSync(join(tmpdir(), "aoe-pw-story-queue-del-"));
  const scriptPath = join(scriptDir, "script.json");
  writeFileSync(scriptPath, JSON.stringify(SCRIPT));

  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    fakeAcpScript: scriptPath,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-queue-del" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const sessionId = sessions[0]!.id;
    await enableCockpitAndWait(serve.baseUrl, sessionId);

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);
    await waitForCockpitView(page);

    // The composer textarea's accessible name changes with `turnActive`;
    // use a regex that matches both placeholder variants so the same
    // locator works idle and mid-turn.
    const composer = page.getByRole("textbox", {
      name: /Send a message|Queue a follow-up/i,
    });
    await composer.fill("kick off");
    await composer.press("Enter");

    await expect(page.getByText("Working on turn 1...")).toBeVisible({
      timeout: 10_000,
    });
    await composer.fill("doomed queued text");
    await page.getByRole("button", { name: /Queue follow-up message/i }).click();

    const queuedRow = page.getByRole("button", { name: /^doomed queued text$/ });
    await expect(queuedRow).toBeVisible({ timeout: 5_000 });

    await page.getByTitle("Drop this queued message").click();
    await expect(queuedRow).toHaveCount(0, { timeout: 5_000 });
  } finally {
    await serve.stop();
    rmSync(scriptDir, { recursive: true, force: true });
  }
});
