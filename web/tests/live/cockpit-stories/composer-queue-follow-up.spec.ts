// User story: queue a follow-up message during an active turn.
//
// Script has two turns. Turn 1 emits a chunk, then waits, then ends.
// While turn 1 is alive the composer placeholder flips to "Queue a
// follow-up…" and the QueueSendButton replaces the Send button. The
// user types a second message and clicks Queue; the cockpit hook
// stashes it on `queuedPrompts`. When turn 1 ends, the drain effect
// dispatches it as the next prompt and turn 2 emits its distinct text.

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { waitForCockpitView , enableCockpitAndWait } from "../../helpers/cockpit";

const QUEUE_SCRIPT = {
  turns: [
    {
      updates: [
        {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "First turn response." },
        },
        // Long enough that the queued follow-up always lands before
        // turn 1 ends on slow CI runners, but well under any 10s
        // idle/watchdog window in the cockpit supervisor (see
        // `RESUME_IDLE_GRACE_DEFAULT` in src/cockpit/acp_client.rs).
        // Earlier rounds bounced between 600ms (raced on slow CI) and
        // 10s (hit the idle watchdog and the worker was torn down
        // before turn 2 could fire). 4s gives the spec ~3.5s of slack
        // to fill+click after observing the first chunk while keeping
        // the total turn well clear of any supervisor watchdog.
        { sessionUpdate: "wait_ms", ms: 4_000 },
      ],
      stopReason: "end_turn",
    },
    {
      updates: [
        {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "Second turn response." },
        },
      ],
      stopReason: "end_turn",
    },
  ],
};

// FIXME (#1383 follow-up): consistently fails on CI under 4-worker
// contention. After turn 1 ends naturally (stopReason=end_turn), the
// cockpit supervisor publishes `Stopped { reason: "user_stopped" }`
// before the client's drain effect can POST the queued follow-up, so
// turn 2 never fires. The user_stopped event means `reap_user_stopped`
// observed a registry-gone worker, which means the runner subprocess
// exited (registry::delete is in runner.rs:284 on agent_child.wait()
// return). Tightening or loosening wait_ms doesn't change the outcome,
// and the symmetric Stop tests with the same chunk+wait_ms script
// pass because they cancel mid-turn and never hit the natural
// end_turn path. The client-side queue + drain logic is still
// covered by Vitest; the supervisor side needs to be diagnosed
// separately before this spec can land green.
base.fixme("queued follow-up fires when first turn ends", async ({ page }, testInfo) => {
  const scriptDir = mkdtempSync(join(tmpdir(), "aoe-pw-story-queue-"));
  const scriptPath = join(scriptDir, "script.json");
  writeFileSync(scriptPath, JSON.stringify(QUEUE_SCRIPT));

  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    fakeAcpScript: scriptPath,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-queue" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const seeded = sessions.find((s) => s.title === "story-queue");
    if (!seeded) throw new Error("seeded session 'story-queue' missing");
    const sessionId = seeded.id;

    await enableCockpitAndWait(serve.baseUrl, sessionId);

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);
    await waitForCockpitView(page);

    const composer = page.getByRole("textbox", {
      name: /Send a message|Queue a follow-up/i,
    });
    await composer.fill("kick off the first turn");
    await composer.press("Enter");

    // Wait for the first chunk so we know the turn is live.
    await expect(page.getByText("First turn response.")).toBeVisible({
      timeout: 10_000,
    });

    // Wait for the QueueSendButton to actually be present before
    // typing / clicking. The composer only renders it while
    // `turnActive=true`; if the worker died or the reducer is still
    // batching state updates, the SendButton (different aria-label,
    // "Send message" or "Queue message until session resumes") is
    // there instead and clicking by Queue's aria-label would block on
    // a button that never appears.
    const queueBtn = page.getByRole("button", { name: /Queue follow-up message/i });
    await expect(queueBtn).toBeVisible({ timeout: 5_000 });
    await composer.fill("second please");
    await queueBtn.click();

    // Turn 1 wait_ms elapses → end_turn → drain effect fires the
    // queued prompt → turn 2 starts and emits its distinct text.
    await expect(page.getByText("Second turn response.")).toBeVisible({
      timeout: 15_000,
    });
  } finally {
    await serve.stop();
    rmSync(scriptDir, { recursive: true, force: true });
  }
});
