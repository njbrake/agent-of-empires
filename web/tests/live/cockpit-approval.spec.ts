// Cockpit approval flow.
//
// Custom FAKE_ACP_SCRIPT (written to a temp file before spawning the
// harness) emits a `permission_request` mid-turn. Seeds the session via
// `aoe add` BEFORE serve boots so the server picks it up in-memory.

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../helpers/aoeServe";
import { enableCockpitAndWait } from "../helpers/cockpit";

const APPROVAL_SCRIPT = {
  turns: [
    {
      updates: [
        {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "Considering write..." },
        },
        {
          // The fake translates this into a real
          // `session/request_permission` JSON-RPC request. ACP has no
          // `permission_request` session/update variant; aoe carries
          // permissions on a separate request that emits an
          // ApprovalRequested event server-side with a server-generated
          // nonce. The spec reads that nonce out of replay below.
          sessionUpdate: "permission_request",
          toolCall: {
            toolCallId: "fake-tool-call-1",
            title: "Write file",
            kind: "edit",
          },
        },
      ],
      stopReason: "end_turn",
    },
  ],
};

interface ApprovalRequestedEvent {
  ApprovalRequested?: {
    approval?: {
      nonce?: string;
    };
  };
}

base("permission_request flows through to the server", async ({}, testInfo) => {
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

    // `cockpit/enable` implicitly spawns the cockpit supervisor;
    // `enableCockpitAndWait` POSTs it and blocks until the ACP
    // handshake (initialize + session/new) completes.
    await enableCockpitAndWait(serve.baseUrl, sessionId);
    await fetch(`${serve.baseUrl}/api/sessions/${sessionId}/cockpit/prompt`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text: "write a file" }),
    });

    // Poll the disk-backed replay endpoint for the ApprovalRequested
    // event aoe emits when the fake agent sends `session/request_permission`.
    // The nonce is generated server-side (src/cockpit/permissions.rs::
    // build_approval), so the spec must read it back instead of
    // hard-coding a value.
    let nonce: string | undefined;
    await expect
      .poll(
        async () => {
          const replay = await fetch(
            `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/replay?since=0`,
          ).then((r) => r.json());
          const frames: Array<{ event?: ApprovalRequestedEvent }> = Array.isArray(
            replay,
          )
            ? replay
            : replay.frames ?? [];
          for (const frame of frames) {
            const candidate = frame.event?.ApprovalRequested?.approval?.nonce;
            if (candidate) {
              nonce = candidate;
              return true;
            }
          }
          return false;
        },
        { timeout: 15_000, intervals: [100, 200, 500, 1000] },
      )
      .toBe(true);
    expect(nonce).toBeDefined();

    // Resolve via the explicit endpoint (UI click path is covered by a
    // follow-up under #1224 once cockpit UI selectors are stable).
    const resolveRes = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/approvals/${nonce}`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        // ApprovalDecisionWire (src/cockpit/protocol.rs) is serialized
        // as PascalCase, so "allow" deserializes as a 422 invalid body.
        body: JSON.stringify({ decision: "Allow" }),
      },
    );
    expect(resolveRes.status).toBeGreaterThanOrEqual(200);
    expect(resolveRes.status).toBeLessThan(300);
  } finally {
    await serve.stop();
    rmSync(scriptDir, { recursive: true, force: true });
  }
});
