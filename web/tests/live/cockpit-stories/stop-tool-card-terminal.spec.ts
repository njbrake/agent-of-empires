// User story: Stopping mid-tool settles the tool card, it does not get
// stuck "running" with a forever-counting timer. See #1646.
//
// A `tool_call` session/update (status=pending) with no completion
// renders a running ToolCard. Before the fix, the card's status was
// derived solely from a paired completion row, so a cancel (which emits
// only a turn-level Stopped, never a per-tool completion) left the card
// orange "running" forever, live and on reload. The reducer now sweeps
// open tool calls to a terminal "stopped" state on any turn-ending
// event, so the badge settles and the timer freezes.

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import {
  waitForCockpitView,
  enableCockpitAndWait,
  attachServeDiagnostics,
} from "../../helpers/cockpit";

const SCRIPT = {
  turns: [
    {
      updates: [
        {
          sessionUpdate: "tool_call",
          toolCallId: "tc-stuck-1",
          title: "Slow tool",
          kind: "read",
          status: "pending",
        },
        { sessionUpdate: "wait_ms", ms: 30_000 },
      ],
      stopReason: "end_turn",
    },
  ],
};

base("stopping mid-tool settles the card and survives reload", async ({ page }, testInfo) => {
  let serveHandle: { home: string } | undefined;
  let serve: Awaited<ReturnType<typeof spawnAoeServe>> | undefined;
  const scriptDir = mkdtempSync(join(tmpdir(), "aoe-pw-stuck-tool-"));
  const scriptPath = join(scriptDir, "script.json");
  writeFileSync(scriptPath, JSON.stringify(SCRIPT));

  try {
    serve = await spawnAoeServe({
      authMode: "none",
      cockpit: true,
      fakeAcpScript: scriptPath,
      workerIndex: testInfo.workerIndex,
      parallelIndex: testInfo.parallelIndex,
      seedFn: seedSessionViaAoeAdd({ title: "story-stuck-tool" }),
    });
    serveHandle = serve;

    const sessions = await listSessions(serve.baseUrl);
    const seeded = sessions.find((s) => s.title === "story-stuck-tool");
    if (!seeded) throw new Error("seeded session 'story-stuck-tool' missing");
    const sessionId = seeded.id;
    await enableCockpitAndWait(serve.baseUrl, sessionId);

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);
    await waitForCockpitView(page);

    const composer = page.getByRole("textbox", { name: /Send a message/i });
    await composer.fill("run a slow tool");
    await composer.press("Enter");

    // The running tool card mounts; the badge is "running" until the
    // turn ends.
    await expect(page.getByText("Slow tool")).toBeVisible({ timeout: 10_000 });
    await expect(page.getByText("running", { exact: true })).toBeVisible({
      timeout: 10_000,
    });

    const stopButton = page.getByRole("button", { name: "Stop" });
    await expect(stopButton).toBeVisible({ timeout: 10_000 });
    await stopButton.click();

    // The card settles to the distinct terminal "stopped" state instead
    // of staying "running" forever. `exact` so the lowercase badge does
    // not match an unrelated "Stopped" banner.
    await expect(page.getByText("stopped", { exact: true })).toBeVisible({
      timeout: 10_000,
    });
    await expect(page.getByText("running", { exact: true })).toBeHidden();

    // The stuck state was persisted before the fix, so a reload must
    // still show the card terminal: the trailing Stopped replays through
    // the same reducer sweep.
    await page.reload();
    await waitForCockpitView(page);
    await expect(page.getByText("Slow tool")).toBeVisible({ timeout: 10_000 });
    await expect(page.getByText("stopped", { exact: true })).toBeVisible({
      timeout: 10_000,
    });
    await expect(page.getByText("running", { exact: true })).toBeHidden();
  } finally {
    try {
      if (serveHandle) await attachServeDiagnostics(testInfo, serveHandle);
    } catch {
      // best-effort diagnostics; do not block cleanup
    }
    try {
      if (serve) await serve.stop();
    } finally {
      rmSync(scriptDir, { recursive: true, force: true });
    }
  }
});
