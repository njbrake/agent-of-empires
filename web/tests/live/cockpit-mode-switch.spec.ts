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

    // `cockpit/enable` implicitly spawns the cockpit supervisor. The
    // endpoint returns 202 the instant the spawn task is queued; the
    // ACP subprocess starts asynchronously. Calling set-mode before
    // the supervisor has registered the session 404s with "session has
    // no running cockpit". A fixed sleep is fragile on slow CI runners
    // (500ms was not enough on GitHub Actions). Poll set-mode until any
    // non-404 means the session is known to the supervisor.
    await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/enable`,
      { method: "POST" },
    );

    let modeRes: Response | undefined;
    for (let attempt = 0; attempt < 30; attempt++) {
      modeRes = await fetch(
        `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/mode`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ mode_id: "plan" }),
        },
      );
      if (modeRes.status !== 404) break;
      await new Promise((r) => setTimeout(r, 200));
    }
    expect(modeRes?.status).toBeGreaterThanOrEqual(200);
    expect(modeRes?.status).toBeLessThan(300);

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
