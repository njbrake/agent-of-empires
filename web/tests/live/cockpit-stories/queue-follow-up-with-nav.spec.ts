// User story: queue a follow-up, navigate to another session, then
// return. The queued prompt should still fire once the first turn
// ends.
//
// Two cockpit-enabled sessions: A and B. Kick off a long turn on A,
// queue a follow-up, navigate to B (CockpitView for A unmounts), then
// back to A. Replay restores the active turn; once it ends, the
// drained follow-up triggers the second turn.

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  resolveAoeBinary,
} from "../../helpers/aoeServe";
import {
  enableCockpitAndWait,
  waitForCockpitView,
} from "../../helpers/cockpit";

const SCRIPT = {
  turns: [
    {
      updates: [
        {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "First turn." },
        },
        // Long enough that the navigation cycle below happens while
        // turn 1 is still alive. 1s left only a sliver of time under
        // CI load.
        { sessionUpdate: "wait_ms", ms: 8_000 },
      ],
      stopReason: "end_turn",
    },
    {
      updates: [
        {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "Second turn after nav." },
        },
      ],
      stopReason: "end_turn",
    },
  ],
};

function seedTwoSessions(): (seedEnv: {
  home: string;
  shimBin: string;
  env: NodeJS.ProcessEnv;
}) => void {
  return ({ home, env }) => {
    for (const [title, subdir] of [
      ["queue-nav-a", "project-a"],
      ["queue-nav-b", "project-b"],
    ] as const) {
      const projectDir = join(home, subdir);
      mkdirSync(projectDir, { recursive: true });
      spawnSync("git", ["init", "-q"], { cwd: projectDir });
      spawnSync("git", ["commit", "--allow-empty", "-q", "-m", "init"], {
        cwd: projectDir,
        env: {
          ...env,
          GIT_AUTHOR_NAME: "t",
          GIT_AUTHOR_EMAIL: "t@t",
          GIT_COMMITTER_NAME: "t",
          GIT_COMMITTER_EMAIL: "t@t",
        },
      });
      const res = spawnSync(
        resolveAoeBinary(),
        ["add", projectDir, "-t", title, "-c", "claude"],
        { env },
      );
      if (res.status !== 0) {
        throw new Error(
          `aoe add ${title} failed: status=${res.status} stderr=${res.stderr?.toString() ?? "<none>"}`,
        );
      }
    }
  };
}

base("queued follow-up fires after navigation away and back", async ({ page }, testInfo) => {
  const scriptDir = mkdtempSync(join(tmpdir(), "aoe-pw-queue-nav-"));
  const scriptPath = join(scriptDir, "script.json");
  writeFileSync(scriptPath, JSON.stringify(SCRIPT));

  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    fakeAcpScript: scriptPath,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedTwoSessions(),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const sessionA = sessions.find((s) => s.title === "queue-nav-a")!;
    const sessionB = sessions.find((s) => s.title === "queue-nav-b")!;

    for (const id of [sessionA.id, sessionB.id]) {
      await enableCockpitAndWait(serve.baseUrl, id);
    }

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionA.id)}`);
    await waitForCockpitView(page);

    const composerA = page.getByRole("textbox", {
      name: /Send a message|Queue a follow-up/i,
    });
    await composerA.fill("kick off A");
    await composerA.press("Enter");
    await expect(page.getByText("First turn.")).toBeVisible({ timeout: 10_000 });

    await composerA.fill("from-after-nav");
    await page.getByRole("button", { name: /Queue follow-up message/i }).click();

    // Navigate away to B, then back to A.
    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionB.id)}`);
    await waitForCockpitView(page);
    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionA.id)}`);
    await waitForCockpitView(page);

    // The first turn ends shortly after; the drained follow-up fires
    // turn 2 and its distinct chunk appears in the transcript.
    await expect(page.getByText("Second turn after nav.")).toBeVisible({
      timeout: 20_000,
    });
  } finally {
    await serve.stop();
    rmSync(scriptDir, { recursive: true, force: true });
  }
});
