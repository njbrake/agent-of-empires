// User story: queue a follow-up message during an active turn.
//
// Script has two turns. Turn 1 emits a chunk, then waits, then ends.
// While turn 1 is alive the composer placeholder flips to "Queue a
// follow-up…" and the QueueSendButton replaces the Send button. The
// user types a second message and clicks Queue; the cockpit hook
// stashes it on `queuedPrompts`. When turn 1 ends, the drain effect
// dispatches it as the next prompt and turn 2 emits its distinct text.

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

const QUEUE_SCRIPT = {
  turns: [
    {
      updates: [
        {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "First turn response." },
        },
        { sessionUpdate: "wait_ms", ms: 600 },
      ],
      stopReason: "end_turn",
    },
    {
      updates: [
        {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "Second turn response." },
        },
      ],
      stopReason: "end_turn",
    },
  ],
};

base("queued follow-up fires when first turn ends", async ({ page }, testInfo) => {
  const scriptDir = mkdtempSync(join(tmpdir(), "aoe-pw-story-queue-"));
  const scriptPath = join(scriptDir, "script.json");
  writeFileSync(scriptPath, JSON.stringify(QUEUE_SCRIPT));

  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    fakeAcpScript: scriptPath,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-queue" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const sessionId = sessions[0]!.id;

    await enableCockpitAndWait(serve.baseUrl, sessionId);

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);
    await waitForCockpitView(page);

    const composer = page.getByRole("textbox", {
      name: /Send a message|Queue a follow-up/i,
    });
    await composer.fill("kick off the first turn");
    await composer.press("Enter");

    // Wait for the first chunk so we know the turn is live.
    await expect(page.getByText("First turn response.")).toBeVisible({
      timeout: 10_000,
    });

    await composer.fill("second please");
    await page.getByRole("button", { name: /Queue follow-up message/i }).click();

    // Turn 1 wait_ms elapses → end_turn → drain effect fires the
    // queued prompt → turn 2 starts and emits its distinct text.
    await expect(page.getByText("Second turn response.")).toBeVisible({
      timeout: 15_000,
    });
  } finally {
    await serve.stop();
    rmSync(scriptDir, { recursive: true, force: true });
  }
});
