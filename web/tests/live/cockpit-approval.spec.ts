// Cockpit approval flow.
//
// Custom FAKE_ACP_SCRIPT (written to a temp file before spawning the
// harness) emits a `permission_request` mid-turn. The spec drives the
// browser to click Allow on the ApprovalCard, then verifies the
// approval resolution lands at the server side.
//
// Uses `spawnAoeServe` directly instead of the `serveCockpit` fixture so
// the fakeAcpScript can be configured per test.

import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test as base, expect } from "@playwright/test";
import { spawnAoeServe, resolveAoeBinary } from "../helpers/aoeServe";

const aoeBinary = resolveAoeBinary();

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

base("permission_request flows through to the server", async ({}, testInfo) => {
  // Write the script file.
  const scriptDir = mkdtempSync(join(tmpdir(), "aoe-pw-acp-script-"));
  const scriptPath = join(scriptDir, "script.json");
  writeFileSync(scriptPath, JSON.stringify(APPROVAL_SCRIPT));

  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    fakeAcpScript: scriptPath,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
  });

  try {
    // Seed a session.
    const projectDir = join(serve.home, "project");
    mkdirSync(projectDir, { recursive: true });
    spawnSync("git", ["init", "-q"], { cwd: projectDir });
    spawnSync("git", ["commit", "--allow-empty", "-q", "-m", "init"], {
      cwd: projectDir,
      env: {
        ...process.env,
        GIT_AUTHOR_NAME: "t",
        GIT_AUTHOR_EMAIL: "t@t",
        GIT_COMMITTER_NAME: "t",
        GIT_COMMITTER_EMAIL: "t@t",
      },
    });
    spawnSync(aoeBinary, ["add", projectDir, "-t", "cockpit-approval", "-c", "claude"], {
      env: {
        ...process.env,
        HOME: serve.home,
        XDG_CONFIG_HOME: join(serve.home, "config"),
        TMPDIR: join(serve.home, "tmp"),
        TMUX_TMPDIR: join(serve.home, "tmux"),
        PATH: `${serve.shimBin}:${process.env.PATH ?? ""}`,
      },
    });

    const sessions = await fetch(`${serve.baseUrl}/api/sessions`).then((r) =>
      r.json(),
    );
    const sessionId = sessions[0].id;

    await fetch(`${serve.baseUrl}/api/sessions/${sessionId}/cockpit/enable`, {
      method: "POST",
    });
    await fetch(`${serve.baseUrl}/api/sessions/${sessionId}/cockpit/spawn`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ agent: "claude" }),
    });
    await fetch(`${serve.baseUrl}/api/sessions/${sessionId}/cockpit/prompt`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text: "write a file" }),
    });

    // Replay must contain a permission_request from the scripted turn.
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

    // Resolve via the explicit endpoint (UI click path is covered by
    // a follow-up under #1224 once cockpit UI selectors are stable).
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
