// User story: fold and unfold a long queued message via the
// "Show full queued prompt" / "Show less" toggle.
//
// A 3+ line queued prompt is considered long; QueuedPromptRow clamps
// the display to `line-clamp-3` and renders a toggle below the
// prompt. Clicking it lifts the clamp without entering edit mode.

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { waitForCockpitView, enableCockpitAndWait } from "../../helpers/cockpit";

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

base("queued long prompt fold and unfold toggle", async ({ page }, testInfo) => {
  const scriptDir = mkdtempSync(join(tmpdir(), "aoe-pw-story-queue-fold-"));
  const scriptPath = join(scriptDir, "script.json");
  writeFileSync(scriptPath, JSON.stringify(SCRIPT));

  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    fakeAcpScript: scriptPath,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-queue-fold" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const seeded = sessions.find((s) => s.title === "story-queue-fold");
    if (!seeded) throw new Error("seeded session 'story-queue-fold' missing");
    const sessionId = seeded.id;
    await enableCockpitAndWait(serve.baseUrl, sessionId);

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);
    await waitForCockpitView(page);

    const composer = page.getByRole("textbox", {
      name: /Send a message|Queue a follow-up/i,
    });
    await composer.fill("kick off");
    await composer.press("Enter");
    await expect(page.getByText("Working on turn 1...")).toBeVisible({
      timeout: 10_000,
    });

    await composer.fill("line 1\nline 2\nline 3 long enough to clamp");
    await page.getByRole("button", { name: /Queue follow-up message/i }).click();

    const expandButton = page.getByRole("button", {
      name: "Show full queued prompt",
    });
    await expect(expandButton).toBeVisible({ timeout: 5_000 });
    await expandButton.click();

    const collapseButton = page.getByRole("button", {
      name: "Collapse queued prompt",
    });
    await expect(collapseButton).toBeVisible({ timeout: 5_000 });
  } finally {
    await serve.stop();
    rmSync(scriptDir, { recursive: true, force: true });
  }
});
