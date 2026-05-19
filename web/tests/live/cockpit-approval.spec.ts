// Cockpit approval flow.
//
// Custom FAKE_ACP_SCRIPT (written to a temp file before spawning the
// harness) emits a `permission_request` mid-turn. Seeds the session via
// `aoe add` BEFORE serve boots so the server picks it up in-memory.

import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../helpers/aoeServe";

const APPROVAL_SCRIPT = {
  turns: [
    {
      updates: [
        {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "Considering write..." },
        },
        {
          sessionUpdate: "permission_request",
          nonce: "fake-nonce-1",
          toolCall: {
            id: "fake-tool-call-1",
            title: "Write file",
            kind: "edit",
          },
        },
      ],
      stopReason: "end_turn",
    },
  ],
};

// Skipped pending #1237. Same underlying issue as cockpit-spawn-prompt:
// supervisor surfaces AgentStartupError before the scripted
// permission_request reaches the replay endpoint.
base.skip("permission_request flows through to the server", async ({}, testInfo) => {
  const scriptDir = mkdtempSync(join(tmpdir(), "aoe-pw-acp-script-"));
  const scriptPath = join(scriptDir, "script.json");
  writeFileSync(scriptPath, JSON.stringify(APPROVAL_SCRIPT));

  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    fakeAcpScript: scriptPath,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "cockpit-approval" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const sessionId = sessions[0]!.id;

    // `cockpit/enable` implicitly spawns the cockpit supervisor.
    await fetch(`${serve.baseUrl}/api/sessions/${sessionId}/cockpit/enable`, {
      method: "POST",
    });
    // Wait for the supervisor to come up before prompting. The spawn is
    // a `tokio::spawn` inside enable and the ACP handshake races the
    // prompt unless we wait long enough for `initialize` + `session/new`
    // to complete. 2s is conservative.
    await new Promise((r) => setTimeout(r, 2_000));
    await fetch(`${serve.baseUrl}/api/sessions/${sessionId}/cockpit/prompt`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text: "write a file" }),
    });

    let sawApproval = false;
    for (let attempt = 0; attempt < 30; attempt++) {
      const replay = await fetch(
        `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/replay?since=0`,
      ).then((r) => r.json());
      const json = JSON.stringify(replay);
      if (
        json.includes("permission_request") ||
        json.includes("ApprovalRequested")
      ) {
        sawApproval = true;
        break;
      }
      await new Promise((r) => setTimeout(r, 200));
    }
    expect(sawApproval).toBe(true);

    // Resolve via the explicit endpoint (UI click path is covered by a
    // follow-up under #1224 once cockpit UI selectors are stable).
    const resolveRes = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/approvals/fake-nonce-1`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ decision: "allow" }),
      },
    );
    expect(resolveRes.status).toBeGreaterThanOrEqual(200);
    expect(resolveRes.status).toBeLessThan(300);
  } finally {
    await serve.stop();
  }
});
