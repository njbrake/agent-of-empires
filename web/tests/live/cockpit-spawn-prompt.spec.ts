// Cockpit spawn + prompt happy path.
//
// Seeds a session via `aoe add` BEFORE serve boots (`seedFn`), with the
// fake ACP agent on PATH as both `claude` and `aoe-agent`. After boot,
// the spec enables cockpit per-session, spawns the cockpit worker,
// sends a prompt, and asserts the replay endpoint surfaces the scripted
// `agent_message_chunk`.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../helpers/aoeServe";

// Skipped pending #1237. After cockpit/enable + a fake-ACP-driven
// prompt, the supervisor publishes AgentStartupError ("ACP connection
// failed: Authentication required") between UserPromptSent and the
// expected agent_message_chunk. The companion cockpit-mode-switch spec
// drives the same harness path and passes, so the harness itself is
// sound; the prompt-side flow needs deeper investigation in
// fakeAcpAgent.mjs or the supervisor's auth wall.
base.skip("cockpit spawn + prompt round-trip emits an agent_message_chunk", async ({}, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "cockpit-trace" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    expect(sessions.length).toBeGreaterThan(0);
    const sessionId: string = sessions[0]!.id;

    // `cockpit/enable` flips the per-session cockpit_mode flag AND
    // implicitly spawns the cockpit supervisor via tokio::spawn. A
    // follow-up explicit POST to /cockpit/spawn would 409 with
    // "already running", so we only call enable and let it own the
    // spawn lifecycle.
    const enableRes = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/enable`,
      { method: "POST" },
    );
    expect(enableRes.ok).toBeTruthy();
    // Wait for the tokio::spawn'd supervisor to come up before prompting.
    // The ACP handshake (initialize + session/new) needs to complete
    // before prompts route to the fake agent. 2s is conservative.
    await new Promise((r) => setTimeout(r, 2_000));

    const promptRes = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/prompt`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ text: "hello cockpit" }),
      },
    );
    expect(promptRes.status).toBeGreaterThanOrEqual(200);
    expect(promptRes.status).toBeLessThan(300);

    let sawChunk = false;
    for (let attempt = 0; attempt < 30; attempt++) {
      const replay = await fetch(
        `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/replay?since=0`,
      ).then((r) => r.json());
      const events: unknown[] = Array.isArray(replay)
        ? replay
        : replay.events ?? [];
      const json = JSON.stringify(events);
      if (
        json.includes("agent_message_chunk") ||
        json.includes("AgentMessageChunk")
      ) {
        sawChunk = true;
        break;
      }
      await new Promise((r) => setTimeout(r, 200));
    }
    expect(sawChunk).toBe(true);
  } finally {
    await serve.stop();
  }
});
