// Cockpit mode switch.
//
// POST /api/sessions/:id/cockpit/mode forwards to the fake ACP agent's
// `session/setMode` handler, which emits `current_mode_changed`. The
// cockpit reducer records it and the replay endpoint surfaces it.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../helpers/aoeServe";
import { waitForReplayContains } from "../helpers/cockpit";

base("session/mode round-trips through the fake ACP agent", async ({}, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "mode-trace" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const sessionId: string = sessions[0]!.id;

    // `cockpit/enable` flips the persisted flag and kicks off the
    // supervisor spawn inside a tokio::spawn. The explicit
    // `cockpit/spawn` below races that async task: if enable's spawn
    // wins, the explicit one returns 409 AlreadyRunning; if explicit
    // wins, it returns 2xx. Either way the supervisor has registered
    // the session by the time the response lands, which is what
    // set-mode needs.
    const enableRes = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/enable`,
      { method: "POST" },
    );
    expect(enableRes.ok).toBeTruthy();
    const spawnRes = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/spawn`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ agent: "claude" }),
      },
    );
    expect([200, 202, 409]).toContain(spawnRes.status);

    const modeRes = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/mode`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ mode_id: "plan" }),
      },
    );
    expect(modeRes.status).toBeGreaterThanOrEqual(200);
    expect(modeRes.status).toBeLessThan(300);

    // The casing-OR survives a future wire-format flip from PascalCase
    // to snake_case (or vice versa). The previous version of this
    // assertion also OR'd in '"plan"' as a sanity check on the target
    // mode, but that's a generic enough substring to false-positive on
    // any frame whose payload happens to contain it (tool args, chat
    // text, etc.), so just match the event name.
    await waitForReplayContains(serve.baseUrl, sessionId, [
      "current_mode_changed",
      "CurrentModeChanged",
    ]);
  } finally {
    await serve.stop();
  }
});
