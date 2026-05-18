// Restart-on-attach e2e for the web dashboard.
//
// Proves the fix in POST /api/sessions/{id}/ensure: the TUI already
// restarts dead agent sessions when the user attaches; this test verifies
// the web path does the same. Runs against a live `aoe serve` backend via
// the shared harness in `../helpers/aoeServe.ts`.
//
// Recipe:
//   1. The harness writes a fake `claude` shim onto an isolated PATH so
//      tmux panes stay alive without a real agent.
//   2. The test runs `aoe add` against the same isolated $HOME to persist
//      a session record without launching tmux (Error status).
//   3. The harness already booted `aoe serve --no-auth`; the test drives
//      it through `/api/sessions/:id/ensure` and tmux assertions.

import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { test, expect } from "../helpers/liveTest";
import { resolveAoeBinary, listSessions } from "../helpers/aoeServe";

const aoeBinary = resolveAoeBinary();

test.describe("ensure_session restart flow", () => {
  function seedSession(home: string, shimBin: string, title: string) {
    const projectDir = join(home, "project");
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
      HOME: home,
      XDG_CONFIG_HOME: join(home, "config"),
      TMPDIR: join(home, "tmp"),
      TMUX_TMPDIR: join(home, "tmux"),
      PATH: `${shimBin}:${process.env.PATH ?? ""}`,
    };
    const addRes = spawnSync(
      aoeBinary,
      ["add", projectDir, "-t", title, "-c", "claude"],
      { env },
    );
    if (addRes.status !== 0) {
      throw new Error(
        `aoe add failed: ${addRes.stderr?.toString() ?? "<no stderr>"}`,
      );
    }
  }

  function tmuxHasSession(home: string, name: string): boolean {
    const res = spawnSync("tmux", ["has-session", "-t", name], {
      env: {
        ...process.env,
        HOME: home,
        TMUX_TMPDIR: join(home, "tmux"),
      },
    });
    return res.status === 0;
  }

  test("dead session is restarted by /ensure, live session stays alive", async ({
    serve,
  }) => {
    const title = "e2e-restart";
    seedSession(serve.home, serve.shimBin, title);

    const sessions = await listSessions(serve.baseUrl);
    expect(sessions.length).toBeGreaterThan(0);
    const sessionId: string = sessions[0]!.id;
    const tmuxName = `aoe_${title}_${sessionId.slice(0, 8)}`;

    expect(sessions[0]!.status).toBe("Error");
    expect(tmuxHasSession(serve.home, tmuxName)).toBe(false);

    const r1 = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/ensure`,
      { method: "POST" },
    );
    expect(r1.ok).toBeTruthy();
    expect((await r1.json()).status).toBe("restarted");
    expect(tmuxHasSession(serve.home, tmuxName)).toBe(true);

    const hookDir = `/tmp/aoe-hooks/${sessionId}`;
    mkdirSync(hookDir, { recursive: true });
    writeFileSync(join(hookDir, "status"), "idle");

    const r2 = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/ensure`,
      { method: "POST" },
    );
    expect((await r2.json()).status).toBe("alive");

    const r3 = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/ensure`,
      { method: "POST" },
    );
    expect((await r3.json()).status).toBe("alive");

    const kill = spawnSync("tmux", ["kill-session", "-t", tmuxName], {
      env: {
        ...process.env,
        HOME: serve.home,
        TMUX_TMPDIR: join(serve.home, "tmux"),
      },
    });
    expect(kill.status).toBe(0);

    const r4 = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/ensure`,
      { method: "POST" },
    );
    expect((await r4.json()).status).toBe("restarted");

    try {
      rmSync(hookDir, { recursive: true, force: true });
    } catch {
      // best-effort
    }
  });

  test("frontend shows Starting placeholder then connects", async ({
    serve,
    page,
  }) => {
    const title = "e2e-restart";
    seedSession(serve.home, serve.shimBin, title);

    const sessions = await listSessions(serve.baseUrl);
    const sessionId: string = sessions[0]!.id;
    const tmuxName = `aoe_${title}_${sessionId.slice(0, 8)}`;

    spawnSync("tmux", ["kill-session", "-t", tmuxName], {
      env: {
        ...process.env,
        HOME: serve.home,
        TMUX_TMPDIR: join(serve.home, "tmux"),
      },
    });

    await page.goto(`${serve.baseUrl}/`);
    const sessionButton = page.getByRole("button", {
      name: /^e2e-restart claude/,
    });
    await expect(sessionButton).toBeVisible();
    await sessionButton.click();

    await expect(page.getByText("Starting session...")).toBeVisible();
    await expect(page.getByText("Starting session...")).toBeHidden({
      timeout: 15_000,
    });
  });
});
