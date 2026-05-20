// Cockpit substrate switch.
//
// `SwitchSubstrateAction` toggles a session between tmux and cockpit
// modes. The two endpoints (`POST /cockpit/enable` and
// `POST /cockpit/disable`) both return
// `{ session_id, cockpit_mode: boolean }` and persist the new substrate
// to the on-disk session record. This spec round-trips both directions
// and asserts the session-list reports the swap on each step.
//
// Independent of #1237: enable returns 200 even when the supervisor's
// async spawn later fails, and disable tears the worker down without
// going through the prompt path.

import { test, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../helpers/aoeServe";

test("substrate switch round-trips between tmux and cockpit", async ({}, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "cockpit-substrate" }),
  });

  try {
    const sessionsBefore = await listSessions(serve.baseUrl);
    const sessionId = sessionsBefore[0]!.id;
    // `aoe add` defaults to tmux mode.
    expect(sessionsBefore[0]!.cockpit_mode).toBeFalsy();

    // tmux -> cockpit
    const enableRes = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/enable`,
      { method: "POST" },
    );
    expect(enableRes.ok).toBeTruthy();
    const enableBody = (await enableRes.json()) as {
      session_id: string;
      cockpit_mode: boolean;
    };
    expect(enableBody.session_id).toBe(sessionId);
    expect(enableBody.cockpit_mode).toBe(true);

    const sessionsAfterEnable = await listSessions(serve.baseUrl);
    expect(
      sessionsAfterEnable.find((s) => s.id === sessionId)!.cockpit_mode,
    ).toBe(true);

    // Idempotent: a second enable returns the same shape without an
    // error and without re-spawning anything destructive.
    const enableAgain = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/enable`,
      { method: "POST" },
    );
    expect(enableAgain.ok).toBeTruthy();
    const enableAgainBody = (await enableAgain.json()) as {
      cockpit_mode: boolean;
    };
    expect(enableAgainBody.cockpit_mode).toBe(true);

    // cockpit -> tmux
    const disableRes = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/disable`,
      { method: "POST" },
    );
    expect(disableRes.ok).toBeTruthy();
    const disableBody = (await disableRes.json()) as {
      session_id: string;
      cockpit_mode: boolean;
    };
    expect(disableBody.cockpit_mode).toBe(false);

    const sessionsAfterDisable = await listSessions(serve.baseUrl);
    expect(
      sessionsAfterDisable.find((s) => s.id === sessionId)!.cockpit_mode,
    ).toBe(false);

    // Idempotent in the other direction too.
    const disableAgain = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/disable`,
      { method: "POST" },
    );
    expect(disableAgain.ok).toBeTruthy();
    const disableAgainBody = (await disableAgain.json()) as {
      cockpit_mode: boolean;
    };
    expect(disableAgainBody.cockpit_mode).toBe(false);
  } finally {
    await serve.stop();
  }
});
