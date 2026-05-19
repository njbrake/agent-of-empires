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

    // `cockpit/enable` flips the persisted cockpit_mode flag and kicks
    // off the supervisor spawn inside a tokio::spawn — the response
    // returns before the subprocess is registered. `cockpit/spawn` is
    // synchronous (awaits the supervisor) so it's the right call when a
    // spec needs the supervisor ready immediately. We use spawn alone:
    // the test only verifies the set-mode round-trip; persisted flag
    // toggling is exercised elsewhere.
    const spawnRes = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/spawn`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ agent: "claude" }),
      },
    );
    expect(spawnRes.status).toBeGreaterThanOrEqual(200);
    expect(spawnRes.status).toBeLessThan(300);

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
