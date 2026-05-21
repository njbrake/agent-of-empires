// User story: live plan progress shows in the sidebar (out-of-session
// view).
//
// Cockpit session emits an ACP `plan` update, the server-side
// reducer turns it into a `plan_summary` on the session record, and
// the sidebar's PlanProgressMini renders a progressbar with an aria
// label embedding completed/total. Navigate to / so the sidebar is
// the primary surface.

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { waitForCockpitReady } from "../../helpers/cockpit";

const PLAN_SCRIPT = {
  turns: [
    {
      updates: [
        {
          sessionUpdate: "plan",
          entries: [
            { content: "Step alpha", status: "in_progress" },
            { content: "Step bravo", status: "pending" },
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

base("sidebar PlanProgressMini renders the cockpit plan summary", async ({ page }, testInfo) => {
  const scriptDir = mkdtempSync(join(tmpdir(), "aoe-pw-sidebar-plan-"));
  const scriptPath = join(scriptDir, "script.json");
  writeFileSync(scriptPath, JSON.stringify(PLAN_SCRIPT));

  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    fakeAcpScript: scriptPath,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-sidebar-plan" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const sessionId = sessions[0]!.id;

    // Enable cockpit and send a prompt via REST so the plan update
    // lands without needing the cockpit view mounted.
    await fetch(`${serve.baseUrl}/api/sessions/${sessionId}/cockpit/enable`, {
      method: "POST",
    });
    await waitForCockpitReady(serve.baseUrl, sessionId);
    await fetch(`${serve.baseUrl}/api/sessions/${sessionId}/cockpit/prompt`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text: "plan it" }),
    });

    await page.goto(serve.baseUrl);
    // Sidebar polls /api/sessions every ~3s; the plan_summary lands
    // shortly after the supervisor processes the prompt.
    await expect(
      page.getByRole("progressbar", {
        name: /Plan progress: 0 of 2 steps/i,
      }),
    ).toBeVisible({ timeout: 20_000 });
  } finally {
    await serve.stop();
    rmSync(scriptDir, { recursive: true, force: true });
  }
});
