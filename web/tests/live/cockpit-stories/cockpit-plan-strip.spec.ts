// User story: agent-emitted plan updates render in the PlanStrip
// above the cockpit transcript.
//
// ACP's `plan` session update carries `entries: [{ content, status,
// priority? }]`. The supervisor translates it to PlanUpdated; the
// reducer applies to state.plan and CockpitView mounts PlanStrip,
// which shows the first in-progress step's title.

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

const PLAN_SCRIPT = {
  turns: [
    {
      updates: [
        {
          sessionUpdate: "plan",
          entries: [
            { content: "Investigate the bug", status: "in_progress" },
            { content: "Write a fix", status: "pending" },
            { content: "Add tests", status: "pending" },
          ],
        },
        {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "Planned." },
        },
      ],
      stopReason: "end_turn",
    },
  ],
};

base("plan session update renders in PlanStrip", async ({ page }, testInfo) => {
  const scriptDir = mkdtempSync(join(tmpdir(), "aoe-pw-story-plan-"));
  const scriptPath = join(scriptDir, "script.json");
  writeFileSync(scriptPath, JSON.stringify(PLAN_SCRIPT));

  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    fakeAcpScript: scriptPath,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-plan" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const sessionId = sessions[0]!.id;
    await enableCockpitAndWait(serve.baseUrl, sessionId);

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);
    await waitForCockpitView(page);

    const composer = page.getByRole("textbox", { name: /Send a message/i });
    await composer.fill("plan this work");
    await composer.press("Enter");

    // PlanStrip shows the current step (first in-progress entry).
    await expect(page.getByText("Investigate the bug")).toBeVisible({
      timeout: 15_000,
    });
    // Progress label "1/3" matches one in-progress + two pending = 0
    // completed; PlanStrip prints completed/total.
    await expect(page.getByText("0/3")).toBeVisible({ timeout: 15_000 });
  } finally {
    await serve.stop();
    rmSync(scriptDir, { recursive: true, force: true });
  }
});
