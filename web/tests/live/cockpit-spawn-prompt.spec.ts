// Cockpit spawn + prompt happy path.
//
// Uses the serveCockpit fixture so $PATH points the cockpit supervisor at
// the fakeAcpAgent ACP shim. The spec:
//   1. Seeds a session via `aoe add`.
//   2. POST /api/sessions/:id/cockpit/spawn  -> 202.
//   3. POST /api/sessions/:id/cockpit/prompt -> 202; fake ACP emits the
//      default scripted update sequence.
//   4. GET /api/sessions/:id/cockpit/replay  -> contains at least one
//      agent_message_chunk update.

import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { test, expect } from "../helpers/liveTest";
import { resolveAoeBinary, listSessions } from "../helpers/aoeServe";

const aoeBinary = resolveAoeBinary();

function seedSession(home: string, shimBin: string, title: string): void {
  const projectDir = join(home, "cockpit-project");
  mkdirSync(projectDir, { recursive: true });
  spawnSync("git", ["init", "-q"], { cwd: projectDir });
  spawnSync("git", ["commit", "--allow-empty", "-q", "-m", "init"], {
    cwd: projectDir,
    env: {
      ...process.env,
      GIT_AUTHOR_NAME: "t",
      GIT_AUTHOR_EMAIL: "t@t",
      GIT_COMMITTER_NAME: "t",
      GIT_COMMITTER_EMAIL: "t@t",
    },
  });
  spawnSync(
    aoeBinary,
    ["add", projectDir, "-t", title, "-c", "claude"],
    {
      env: {
        ...process.env,
        HOME: home,
        XDG_CONFIG_HOME: join(home, "config"),
        TMPDIR: join(home, "tmp"),
        TMUX_TMPDIR: join(home, "tmux"),
        PATH: `${shimBin}:${process.env.PATH ?? ""}`,
      },
    },
  );
}

test("cockpit spawn + prompt round-trip emits an agent_message_chunk", async ({
  serveCockpit,
}) => {
  seedSession(serveCockpit.home, serveCockpit.shimBin, "cockpit-trace");

  const sessions = await listSessions(serveCockpit.baseUrl);
  expect(sessions.length).toBeGreaterThan(0);
  const sessionId: string = sessions[0]!.id;

  // Enable cockpit mode on the session, then spawn.
  const enableRes = await fetch(
    `${serveCockpit.baseUrl}/api/sessions/${sessionId}/cockpit/enable`,
    { method: "POST" },
  );
  expect(enableRes.ok).toBeTruthy();

  const spawnRes = await fetch(
    `${serveCockpit.baseUrl}/api/sessions/${sessionId}/cockpit/spawn`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ agent: "claude" }),
    },
  );
  expect(spawnRes.status).toBeGreaterThanOrEqual(200);
  expect(spawnRes.status).toBeLessThan(300);

  // Send a prompt; the fake ACP emits the default scripted turn.
  const promptRes = await fetch(
    `${serveCockpit.baseUrl}/api/sessions/${sessionId}/cockpit/prompt`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text: "hello cockpit" }),
    },
  );
  expect(promptRes.status).toBeGreaterThanOrEqual(200);
  expect(promptRes.status).toBeLessThan(300);

  // Replay should contain at least one agent_message_chunk event from the
  // scripted turn within a few seconds.
  let sawChunk = false;
  for (let attempt = 0; attempt < 30; attempt++) {
    const replay = await fetch(
      `${serveCockpit.baseUrl}/api/sessions/${sessionId}/cockpit/replay?since=0`,
    ).then((r) => r.json());
    const events: unknown[] = Array.isArray(replay) ? replay : replay.events ?? [];
    const json = JSON.stringify(events);
    if (json.includes("agent_message_chunk") || json.includes("AgentMessageChunk")) {
      sawChunk = true;
      break;
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  expect(sawChunk).toBe(true);
});
