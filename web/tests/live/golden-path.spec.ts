// Golden-path live spec: the single most-valuable end-to-end flow.
//
// Set up a session via the real backend, verify it shows up in the
// sidebar, click into it and see the terminal pane mount, then delete
// it through the API and assert it's gone. API-for-setup-UI-for-verify
// keeps the test focused on the failure modes Playwright is uniquely
// good at (rendering, routing, sidebar state).
//
// Wizard-driven creation is exercised by mocked specs (wizard-*.spec.ts)
// and will be covered live in #1219.

import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { test, expect } from "../helpers/liveTest";
import { resolveAoeBinary } from "../helpers/aoeServe";

const aoeBinary = resolveAoeBinary();

test("create, view, delete a session via live backend", async ({
  serve,
  page,
}) => {
  const projectDir = join(serve.home, "golden-path-project");
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

  const env = {
    ...process.env,
    HOME: serve.home,
    XDG_CONFIG_HOME: join(serve.home, "config"),
    TMPDIR: join(serve.home, "tmp"),
    TMUX_TMPDIR: join(serve.home, "tmux"),
    PATH: `${serve.shimBin}:${process.env.PATH ?? ""}`,
  };
  const addRes = spawnSync(
    aoeBinary,
    ["add", projectDir, "-t", "golden", "-c", "claude"],
    { env },
  );
  expect(addRes.status).toBe(0);

  // The session record now exists on disk and the live serve picks it
  // up on the next /api/sessions GET. Confirm via API before driving UI.
  const sessions = await fetch(`${serve.baseUrl}/api/sessions`).then((r) =>
    r.json(),
  );
  expect(sessions.length).toBeGreaterThan(0);
  const sessionId: string = sessions[0].id;

  await page.goto(`${serve.baseUrl}/`);
  const sessionRow = page.getByRole("button", { name: /^golden claude/ });
  await expect(sessionRow).toBeVisible({ timeout: 10_000 });

  await sessionRow.click();
  // Either the terminal mounts (Starting placeholder appears then unmounts)
  // or the placeholder appears at all; both are valid signals that the
  // session view took over from the empty state.
  await expect(page.getByText("Starting session...")).toBeVisible({
    timeout: 10_000,
  });

  // Delete via API; sidebar should remove the row.
  const deleteRes = await fetch(
    `${serve.baseUrl}/api/sessions/${sessionId}`,
    { method: "DELETE" },
  );
  expect(deleteRes.ok).toBeTruthy();
  await expect(sessionRow).toBeHidden({ timeout: 10_000 });

  const after = await fetch(`${serve.baseUrl}/api/sessions`).then((r) =>
    r.json(),
  );
  expect(after.find((s: { id: string }) => s.id === sessionId)).toBeUndefined();
});
