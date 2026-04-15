// Restart-on-attach e2e for the web dashboard.
//
// Proves the fix in POST /api/sessions/{id}/ensure: the TUI already restarts
// dead agent sessions when the user attaches; this test verifies the web path
// does the same. It runs against a live `aoe serve` backend (not the vite
// preview static server) so the real end-to-end flow is exercised.
//
// Recipe mirrors AGENTS.md §"Web Dashboard Playwright Tests":
//   1. Shim a fake `claude` binary on $PATH that execs `bash -i`, giving the
//      tmux pane a long-running shell (no real agent required).
//   2. `aoe add --cmd claude` to persist a session record.
//   3. `aoe serve --no-auth --port N` subprocess, isolated $HOME.
//   4. Drive the live backend via fetch() + kill tmux between calls.

import { test, expect } from "@playwright/test";
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { mkdtempSync, writeFileSync, chmodSync, mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import net from "node:net";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

test.describe("ensure_session restart flow", () => {
  let serverProc: ChildProcess | null = null;
  let baseUrl = "";
  let sessionId = "";
  let tmuxName = "";
  let sandboxHome = "";

  const aoeBinary = join(__dirname, "..", "..", "target", "release", "aoe");

  async function freePort(): Promise<number> {
    return new Promise((resolve, reject) => {
      const srv = net.createServer();
      srv.unref();
      srv.on("error", reject);
      srv.listen(0, () => {
        const addr = srv.address();
        if (typeof addr === "object" && addr) {
          const port = addr.port;
          srv.close(() => resolve(port));
        } else {
          reject(new Error("no port"));
        }
      });
    });
  }

  async function waitForServer(url: string, timeoutMs: number) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      try {
        const res = await fetch(`${url}/api/sessions`);
        if (res.ok) return;
      } catch {
        // not up yet
      }
      await new Promise((r) => setTimeout(r, 100));
    }
    throw new Error(`aoe serve did not become ready within ${timeoutMs}ms`);
  }

  test.beforeAll(async () => {
    sandboxHome = mkdtempSync(join(tmpdir(), "aoe-e2e-"));
    const home = join(sandboxHome, "home");
    const xdg = join(home, ".config");
    const binDir = join(sandboxHome, "bin");
    const projectDir = join(sandboxHome, "project");
    mkdirSync(home);
    mkdirSync(xdg, { recursive: true });
    mkdirSync(binDir);
    mkdirSync(projectDir);

    const shimPath = join(binDir, "claude");
    // Agent shim must keep the pane alive without leaving a shell as its
    // current process: tmux `remain-on-exit on` means a dead pane sticks
    // around and trips the `pane_dead` branch. `exec tail -f /dev/null`
    // ensures pane_current_command is "tail" (not a shell) in steady state.
    writeFileSync(
      shimPath,
      "#!/bin/bash\nexec tail -f /dev/null\n",
    );
    chmodSync(shimPath, 0o755);

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
      XDG_CONFIG_HOME: xdg,
      PATH: `${binDir}:/usr/local/bin:/usr/bin:/bin`,
      // Leave AGENT_OF_EMPIRES_DEBUG unset by default to keep the run quiet;
      // set it via `AGENT_OF_EMPIRES_DEBUG=1 npx playwright test ...` if the
      // test fails and you need the debug.log trail.
      ...(process.env.AGENT_OF_EMPIRES_DEBUG
        ? { AGENT_OF_EMPIRES_DEBUG: process.env.AGENT_OF_EMPIRES_DEBUG }
        : {}),
    };

    const addRes = spawnSync(
      aoeBinary,
      [
        "add",
        projectDir,
        "-t",
        "e2e-restart",
        "-c",
        "claude",
      ],
      { env },
    );
    if (addRes.status !== 0) {
      throw new Error(
        `aoe add failed: ${addRes.stderr?.toString() ?? "<no stderr>"}`,
      );
    }

    const port = await freePort();
    baseUrl = `http://127.0.0.1:${port}`;
    serverProc = spawn(
      aoeBinary,
      ["serve", "--host", "127.0.0.1", "--port", String(port), "--no-auth"],
      { env, stdio: "pipe" },
    );
    serverProc.on("error", (e) => {
      console.error("[aoe serve] spawn error:", e);
    });

    await waitForServer(baseUrl, 10_000);

    const listRes = await fetch(`${baseUrl}/api/sessions`);
    const sessions = await listRes.json();
    if (!Array.isArray(sessions) || sessions.length === 0) {
      throw new Error("expected one session in isolated profile");
    }
    sessionId = sessions[0].id;
    // tmux session name = aoe_{sanitize(title)}_{id[0..8]}. sanitize keeps
    // alphanumerics, `-`, `_` and replaces everything else with `_`, so
    // "e2e-restart" passes through unchanged.
    tmuxName = `aoe_e2e-restart_${sessionId.slice(0, 8)}`;
  });

  test.afterAll(async () => {
    // Each cleanup step is guarded so partial setup from a failing
    // `beforeAll` still tears down what was created.
    try {
      if (serverProc && !serverProc.killed) {
        serverProc.kill("SIGKILL");
      }
    } catch {
      // server already gone
    }
    if (process.env.AGENT_OF_EMPIRES_DEBUG && sandboxHome) {
      try {
        const logPath = join(
          sandboxHome,
          "home",
          ".config",
          "agent-of-empires",
          "debug.log",
        );
        const log = (await import("node:fs")).readFileSync(logPath, "utf8");
        const tail = log.split("\n").slice(-40).join("\n");
        console.log(`[aoe debug.log tail]\n${tail}`);
      } catch {
        // log absent
      }
    }
    if (sandboxHome) {
      try {
        spawnSync("tmux", ["kill-server"], {
          env: { ...process.env, HOME: join(sandboxHome, "home") },
        });
      } catch {
        // tmux not running or already torn down
      }
      try {
        rmSync(sandboxHome, { recursive: true, force: true });
      } catch {
        // best-effort
      }
    }
    if (sessionId) {
      try {
        rmSync(`/tmp/aoe-hooks/${sessionId}`, {
          recursive: true,
          force: true,
        });
      } catch {
        // best-effort
      }
    }
  });

  // Helper: query tmux for session presence, scoped to the test's HOME so we
  // don't see other sessions on the dev machine.
  function tmuxHasSession(name: string): boolean {
    const res = spawnSync("tmux", ["has-session", "-t", name], {
      env: { ...process.env, HOME: join(sandboxHome, "home") },
    });
    return res.status === 0;
  }

  test("dead session is restarted by /ensure, live session stays alive", async () => {
    // Initial state: aoe add persists the record but does not launch tmux, so
    // the background poller has marked the session Error ("tmux session is
    // gone"). Belt + suspenders: also assert tmux has no session, so this
    // test still tests the dead path even if `aoe add` ever changes to
    // auto-launch (we'd notice the assertion flip immediately).
    const before = await fetch(`${baseUrl}/api/sessions`).then((r) => r.json());
    expect(before[0].status).toBe("Error");
    expect(tmuxHasSession(tmuxName)).toBe(false);

    // First /ensure: session is dead, must restart.
    const r1 = await fetch(`${baseUrl}/api/sessions/${sessionId}/ensure`, {
      method: "POST",
    });
    expect(r1.ok).toBeTruthy();
    const body1 = await r1.json();
    expect(body1.status).toBe("restarted");
    expect(tmuxHasSession(tmuxName)).toBe(true);

    // Real agents write a hook status file so AoE can read authoritative
    // status. The path layout is owned by `crate::hooks::status_file` —
    // `HOOK_STATUS_BASE` (`/tmp/aoe-hooks`) joined with the instance id;
    // see `src/hooks/status_file.rs::hook_status_dir` for the contract.
    // With a hook status file present, ensure's shell-detection fallback
    // short-circuits (hook-tracked sessions can't be judged by pane cmd),
    // which avoids racing with the wrapper bash before it execs the agent.
    const fs = await import("node:fs");
    const hookDir = `/tmp/aoe-hooks/${sessionId}`;
    fs.mkdirSync(hookDir, { recursive: true });
    fs.writeFileSync(join(hookDir, "status"), "idle");

    // Second /ensure: hook status is tracked AND tmux session exists. Must
    // report alive, NOT restart. This is the core bug the PR fixes — without
    // the tmux-format separator fix in src/tmux/mod.rs (tmux 3.4 mangles `\t`
    // to `_` in -F output), refresh_session_cache would silently miss the
    // live session, making exists() return false and forcing a spurious
    // restart that fails with "duplicate session".
    const r2 = await fetch(`${baseUrl}/api/sessions/${sessionId}/ensure`, {
      method: "POST",
    });
    expect(r2.ok).toBeTruthy();
    const body2 = await r2.json();
    expect(body2.status).toBe("alive");

    // Third /ensure, same state, must still be alive (idempotent).
    const r3 = await fetch(`${baseUrl}/api/sessions/${sessionId}/ensure`, {
      method: "POST",
    });
    expect(r3.ok).toBeTruthy();
    expect((await r3.json()).status).toBe("alive");

    // Kill the tmux session externally (simulates the agent process exiting
    // or the user running `tmux kill-session` from another terminal).
    const kill = spawnSync("tmux", ["kill-session", "-t", tmuxName], {
      env: { ...process.env, HOME: join(sandboxHome, "home") },
    });
    expect(kill.status).toBe(0);

    // Next /ensure: must detect the missing session and restart it.
    const r4 = await fetch(`${baseUrl}/api/sessions/${sessionId}/ensure`, {
      method: "POST",
    });
    expect(r4.ok).toBeTruthy();
    const body4 = await r4.json();
    expect(body4.status).toBe("restarted");
  });

  test("frontend shows Starting placeholder then connects", async ({
    page,
  }) => {
    // Pre-kill the session so the frontend hits the restart path when it
    // ensures before opening the WebSocket.
    spawnSync("tmux", ["kill-session", "-t", tmuxName], {
      env: { ...process.env, HOME: join(sandboxHome, "home") },
    });

    await page.goto(`${baseUrl}/`);
    // Session list item is a button; aria name includes the agent + tmux
    // indicator (see Sidebar render). Matching the button is unique.
    const sessionButton = page.getByRole("button", {
      name: /^e2e-restart claude/,
    });
    await expect(sessionButton).toBeVisible();
    await sessionButton.click();

    // The TerminalView shows "Starting session..." while ensureSession() is
    // restarting the tmux session, then swaps in the xterm container once
    // the WebSocket is ready.
    await expect(page.getByText("Starting session...")).toBeVisible();
    // Once ensureSession() resolves, the "Starting session..." placeholder
    // unmounts and xterm takes over. The dashboard also renders a paired
    // host-terminal in the right split, so there are two xterm instances —
    // just wait for the placeholder to go away.
    await expect(page.getByText("Starting session...")).toBeHidden({
      timeout: 15_000,
    });
  });
});
