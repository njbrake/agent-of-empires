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
import { enableCockpitAndWait, waitForReplayContains } from "../helpers/cockpit";

base("cockpit spawn + prompt round-trip emits an agent_message_chunk", async ({}, testInfo) => {
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
    // spawn lifecycle. `enableCockpitAndWait` POSTs enable, asserts a
    // 2xx, then waits for the ACP handshake (initialize + session/new)
    // to finish before returning.
    await enableCockpitAndWait(serve.baseUrl, sessionId);

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

    // Match either casing in case the wire format moves to snake_case
    // (frames currently serialize `event` as an externally-tagged enum,
    // keyed `AgentMessageChunk`; src/server/api/cockpit.rs::cockpit_replay).
    await waitForReplayContains(serve.baseUrl, sessionId, [
      "agent_message_chunk",
      "AgentMessageChunk",
    ]);
  } finally {
    await serve.stop();
  }
});
