// Cockpit shutdown via DELETE.
//
// `DELETE /api/sessions/:id/cockpit` calls `supervisor.shutdown(&id)`
// to tear down the cockpit worker subprocess. Returns 204 on success,
// 404 when the supervisor has no entry for the session.
//
// Distinct from `POST /cockpit/disable`, which also swaps substrate
// back to tmux. This endpoint only stops the worker; substrate state
// (cockpit_mode) is preserved so a subsequent
// `POST /cockpit/spawn` can re-attach without re-enabling.

import { test, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../helpers/aoeServe";

test("DELETE /cockpit shuts the worker down with 204 / 404", async ({}, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "cockpit-shutdown" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const sessionId = sessions[0]!.id;

    // Pre-enable: no worker registered yet, so DELETE returns 404. The
    // server distinguishes "session not found" from "session has no
    // running cockpit"; this branch is the latter.
    const preDelete = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit`,
      { method: "DELETE" },
    );
    expect(preDelete.status).toBe(404);

    // Bring the worker up. The supervisor's spawn is `tokio::spawn`'d
    // inside enable, so the worker entry may not yet exist when enable
    // returns; poll up to 5s for the registry insert.
    await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/enable`,
      { method: "POST" },
    );

    let postDeleteStatus = 0;
    for (let attempt = 0; attempt < 25; attempt++) {
      const res = await fetch(
        `${serve.baseUrl}/api/sessions/${sessionId}/cockpit`,
        { method: "DELETE" },
      );
      postDeleteStatus = res.status;
      if (res.status === 204) break;
      await new Promise((r) => setTimeout(r, 200));
    }
    expect(postDeleteStatus).toBe(204);

    // After shutdown the supervisor entry is gone, so a second DELETE
    // is back to 404.
    const repeat = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit`,
      { method: "DELETE" },
    );
    expect(repeat.status).toBe(404);

    // Substrate state survives the worker teardown: cockpit_mode is
    // still true on the session record. That's the contract that
    // distinguishes shutdown from disable.
    const after = await listSessions(serve.baseUrl);
    expect(after.find((s) => s.id === sessionId)!.cockpit_mode).toBe(true);
  } finally {
    await serve.stop();
  }
});
