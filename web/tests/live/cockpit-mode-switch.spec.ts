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

// Skipped pending #1237: the supervisor's ACP handshake against the
// fake agent fails with "Authentication required" before the worker
// registers, so set-mode 404s. Sibling cockpit live specs
// (cockpit-spawn-prompt, cockpit-approval) are skipped for the same
// reason. Unskip once the harness installs a `claude-agent-acp` shim
// (or the supervisor's agent registry honors an env-var override) so
// the fake ACP agent actually drives the handshake.
base.skip("session/mode round-trips through the fake ACP agent", async ({}, testInfo) => {
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

    let sawModeChange = false;
    for (let attempt = 0; attempt < 30; attempt++) {
      const replay = await fetch(
        `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/replay?since=0`,
      ).then((r) => r.json());
      const json = JSON.stringify(replay);
      if (
        json.includes("current_mode_changed") ||
        json.includes("CurrentModeChanged") ||
        json.includes("\"plan\"")
      ) {
        sawModeChange = true;
        break;
      }
      await new Promise((r) => setTimeout(r, 200));
    }
    expect(sawModeChange).toBe(true);
  } finally {
    await serve.stop();
  }
});
