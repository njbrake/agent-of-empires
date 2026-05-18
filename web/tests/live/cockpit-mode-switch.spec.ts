// Cockpit mode switch.
//
// POST /api/sessions/:id/cockpit/mode forwards the requested SessionMode
// to the fake ACP agent, which echoes back a `current_mode_changed`
// update. The cockpit reducer (server-side state machine) records the
// new mode id and surfaces it via replay.

import { mkdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { test, expect } from "../helpers/liveTest";
import { resolveAoeBinary, listSessions } from "../helpers/aoeServe";

const aoeBinary = resolveAoeBinary();

test("session/mode round-trips through the fake ACP agent", async ({
  serveCockpit,
}) => {
  const projectDir = join(serveCockpit.home, "mode-project");
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
  spawnSync(aoeBinary, ["add", projectDir, "-t", "mode-trace", "-c", "claude"], {
    env: {
      ...process.env,
      HOME: serveCockpit.home,
      XDG_CONFIG_HOME: join(serveCockpit.home, "config"),
      TMPDIR: join(serveCockpit.home, "tmp"),
      TMUX_TMPDIR: join(serveCockpit.home, "tmux"),
      PATH: `${serveCockpit.shimBin}:${process.env.PATH ?? ""}`,
    },
  });

  const sessions = await listSessions(serveCockpit.baseUrl);
  const sessionId: string = sessions[0]!.id;

  await fetch(
    `${serveCockpit.baseUrl}/api/sessions/${sessionId}/cockpit/enable`,
    { method: "POST" },
  );
  await fetch(
    `${serveCockpit.baseUrl}/api/sessions/${sessionId}/cockpit/spawn`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ agent: "claude" }),
    },
  );

  const modeRes = await fetch(
    `${serveCockpit.baseUrl}/api/sessions/${sessionId}/cockpit/mode`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ mode: "plan" }),
    },
  );
  expect(modeRes.status).toBeGreaterThanOrEqual(200);
  expect(modeRes.status).toBeLessThan(300);

  // Replay should reflect the new current mode.
  let sawModeChange = false;
  for (let attempt = 0; attempt < 30; attempt++) {
    const replay = await fetch(
      `${serveCockpit.baseUrl}/api/sessions/${sessionId}/cockpit/replay?since=0`,
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
});
