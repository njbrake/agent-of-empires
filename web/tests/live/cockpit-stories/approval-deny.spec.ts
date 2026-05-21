// User story: clicking Deny on an ApprovalCard resolves the request
// and the turn ends.
//
// Script emits a permission_request and then end_turn. After Deny, the
// fake's session/request_permission promise resolves with the reject
// decision and the turn finishes with no further chunks. The composer
// flips back to the idle "Send a message…" placeholder.

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { waitForCockpitReady, waitForCockpitView } from "../../helpers/cockpit";

const DENY_SCRIPT = {
  turns: [
    {
      updates: [
        {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "Asking permission..." },
        },
        {
          sessionUpdate: "permission_request",
          toolCall: {
            toolCallId: "fake-tool-call-deny",
            title: "Delete file",
            kind: "edit",
          },
        },
      ],
      stopReason: "end_turn",
    },
  ],
};

base("ApprovalCard Deny resolves and the turn ends", async ({ page }, testInfo) => {
  const scriptDir = mkdtempSync(join(tmpdir(), "aoe-pw-story-deny-"));
  const scriptPath = join(scriptDir, "script.json");
  writeFileSync(scriptPath, JSON.stringify(DENY_SCRIPT));

  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    fakeAcpScript: scriptPath,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-deny" }),
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
    await composer.fill("please delete something");
    await composer.press("Enter");

    const approvalDialog = page.getByRole("alertdialog", {
      name: /Approval needed/i,
    });
    await expect(approvalDialog).toBeVisible({ timeout: 10_000 });

    await approvalDialog.getByRole("button", { name: "Deny" }).click();

    await expect(approvalDialog).toBeHidden({ timeout: 10_000 });
    // Turn ends; idle composer placeholder reappears, Stop button gone.
    await expect(
      page.getByRole("textbox", { name: /Send a message/i }),
    ).toBeVisible({ timeout: 10_000 });
    await expect(page.getByRole("button", { name: "Stop" })).toBeHidden({
      timeout: 10_000,
    });
  } finally {
    await serve.stop();
    rmSync(scriptDir, { recursive: true, force: true });
  }
});
