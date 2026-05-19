// Live-backend test harness for Playwright.
//
// `spawnAoeServe()` boots a real `aoe serve` subprocess against an isolated
// filesystem root (`HOME`, `XDG_CONFIG_HOME`, `TMPDIR`, `TMUX_TMPDIR`) and a
// per-worker port range, returns a `ServeHandle`, and cleans up after the
// test via `stop()`. Designed for fresh-process-per-test isolation: each
// test gets its own root, its own port, its own tmux socket.
//
// Worker isolation: callers pass `workerIndex` and `parallelIndex` (from
// Playwright's `testInfo`). Port and TMUX_TMPDIR are derived deterministically
// so parallel workers never collide. tmux is contained inside the test's
// HOME tree, so cleanup is a simple `rm -rf home`.
//
// See `docs/development/playwright.md` for the full recipe.

import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import {
  existsSync,
  mkdtempSync,
  writeFileSync,
  chmodSync,
  mkdirSync,
  realpathSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { randomBytes } from "node:crypto";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const DEFAULT_PASSPHRASE = "aoe-e2e-fixed-passphrase";

export type AuthMode = "none" | "passphrase";

export interface SpawnOptions {
  authMode?: AuthMode;
  readOnly?: boolean;
  passphrase?: string;
  workerIndex: number;
  parallelIndex: number;
  /** Extra args to pass after the base `aoe serve` flags. */
  extraArgs?: string[];
  /** Override the spawn timeout (default 10s). */
  spawnTimeoutMs?: number;
  /**
   * When true, install `fakeAcpAgent.mjs` as the `claude` / `aoe-agent`
   * shim instead of the tail-f-dev-null stub, and flip the cockpit
   * master enable flag via `PATCH /api/cockpit/master` after the server
   * boots.
   */
  cockpit?: boolean;
  /** Optional path to a FAKE_ACP_SCRIPT for cockpit tests. */
  fakeAcpScript?: string;
  /**
   * Runs after the isolated $HOME tree is set up and the fake shim is on
   * PATH, but BEFORE `aoe serve` spawns. Use to call `aoe add` so the
   * server picks up the session record in-memory on boot (a post-spawn
   * `aoe add` would write to disk but the running server's
   * `state.instances` cache would never reload). The callback receives
   * the same env vars the server will run with, ready to pass straight
   * to `child_process.spawnSync(..., { env: seedEnv.env })`.
   */
  seedFn?: (seedEnv: {
    home: string;
    shimBin: string;
    xdg: string;
    tmp: string;
    tmuxTmp: string;
    env: NodeJS.ProcessEnv;
  }) => void | Promise<void>;
}

export interface ServeHandle {
  baseUrl: string;
  port: number;
  /** Root of the isolated filesystem tree (HOME / XDG / TMPDIR / TMUX_TMPDIR). */
  home: string;
  /** Directory prepended to PATH (contains the fake `claude` shim). */
  shimBin: string;
  proc: ChildProcess;
  authMode: AuthMode;
  passphrase?: string;
  /**
   * Set when `authMode === "passphrase"` and the harness has minted a
   * session via POST /api/login. Callers (typically the Playwright fixture)
   * inject this cookie into the browser context before navigation.
   */
  sessionCookie?: { name: string; value: string };
  /**
   * Stable base64url device binding secret the harness used at login time.
   * Specs that drive auth flows from the browser side need to seed the
   * same value into `localStorage` under `aoe-device-binding-secret`.
   */
  deviceBindingSecret?: string;
  /**
   * The tmux session prefix the running binary uses. Debug-mode builds
   * (`debug_assertions=true`, set by both `cargo build` and `cargo build
   * --profile dev-release`) use `aoe_dev_`; release builds use `aoe_`.
   * Specs that need to assert on tmux session names should compose this
   * with the session title rather than hard-coding `aoe_`.
   */
  tmuxPrefix: "aoe_" | "aoe_dev_";
  stop(): Promise<void>;
}

/**
 * Fetch and unwrap `GET /api/sessions`. As of #1171 the response shape is
 * `{ sessions: SessionResponse[], workspace_ordering: string[] }`. Callers
 * typically want only the sessions array, so this helper hides the
 * envelope change so a future shape tweak is one edit away.
 */
export async function listSessions(
  baseUrl: string,
): Promise<Array<{ id: string; title: string; status: string; [k: string]: unknown }>> {
  const res = await fetch(`${baseUrl}/api/sessions`);
  if (!res.ok) {
    throw new Error(`GET /api/sessions failed: ${res.status} ${await res.text()}`);
  }
  const body = await res.json();
  if (Array.isArray(body)) return body;
  if (body && Array.isArray(body.sessions)) return body.sessions;
  throw new Error(
    `GET /api/sessions returned an unexpected shape: ${JSON.stringify(body).slice(0, 200)}`,
  );
}

/**
 * Returns a `seedFn` for `spawnAoeServe` that:
 *   1. git-inits a fresh project dir under the isolated HOME.
 *   2. runs `aoe add <projectDir> -t <title> -c <tool>` against the same env.
 *
 * Must run BEFORE serve spawns so the server picks up the session record
 * in-memory on boot. A post-spawn `aoe add` writes to disk but the running
 * server's `state.instances` cache never reloads, so subsequent
 * `GET /api/sessions` returns an empty list.
 */
export function seedSessionViaAoeAdd(opts: {
  title: string;
  tool?: string;
  subdir?: string;
}): (seedEnv: {
  home: string;
  shimBin: string;
  env: NodeJS.ProcessEnv;
}) => void {
  return ({ home, env }) => {
    const projectDir = join(home, opts.subdir ?? "project");
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
    const addRes = spawnSync(
      resolveAoeBinary(),
      ["add", projectDir, "-t", opts.title, "-c", opts.tool ?? "claude"],
      { env },
    );
    if (addRes.status !== 0) {
      throw new Error(
        `aoe add failed: status=${addRes.status} stderr=${addRes.stderr?.toString() ?? "<none>"}`,
      );
    }
  };
}

export function resolveAoeBinary(): string {
  const fromEnv = process.env.AOE_E2E_BINARY;
  if (fromEnv && existsSync(fromEnv)) return fromEnv;
  const repoRoot = resolve(__dirname, "..", "..", "..");
  // Prefer release if both exist (CI builds release by default), fall
  // back to debug for local `cargo build` flows.
  const release = join(repoRoot, "target", "release", "aoe");
  if (existsSync(release)) return release;
  return join(repoRoot, "target", "debug", "aoe");
}

/**
 * Map a resolved aoe binary path to the tmux session prefix the binary
 * will use. The Rust side sets the prefix at compile time based on
 * `cfg!(debug_assertions)`; we can't query it from JS, so we derive it
 * from the build directory in the path. CI passes the binary via
 * `AOE_E2E_BINARY` so this works in CI; locally it falls through to the
 * release/debug heuristic in `resolveAoeBinary`.
 */
export function tmuxPrefixFor(binaryPath: string): "aoe_" | "aoe_dev_" {
  return binaryPath.includes("/target/debug/") ? "aoe_dev_" : "aoe_";
}

function portFor(workerIndex: number, parallelIndex: number, attempt: number): number {
  // 5200 + worker*100 + parallel + attempt*7 covers ~14 retries per
  // (worker, parallel) slot before colliding with the next slot.
  return 5200 + workerIndex * 100 + parallelIndex + attempt * 7;
}

async function waitForServer(
  baseUrl: string,
  deadlineMs: number,
  proc: ChildProcess,
  authMode: AuthMode,
): Promise<void> {
  const deadline = Date.now() + deadlineMs;
  let lastErr: unknown = "no attempts made";
  while (Date.now() < deadline) {
    if (proc.exitCode !== null || proc.signalCode !== null) {
      throw new Error(
        `aoe serve died before ready (exit=${proc.exitCode} signal=${proc.signalCode})`,
      );
    }
    try {
      const res = await fetch(`${baseUrl}/api/about`);
      // In `--no-auth` mode the server returns 200 outright. In passphrase
      // mode it returns 401 BUT also sets a distinct WWW-Authenticate-ish
      // response shape. Accepting 401 here without distinguishing makes
      // the harness latch onto stale token-auth servers that other test
      // runs left running on the same port. Be precise per authMode.
      if (authMode === "none" && res.status === 200) return;
      if (authMode === "passphrase" && (res.status === 200 || res.status === 401))
        return;
      lastErr = `status ${res.status}`;
    } catch (err) {
      lastErr = err;
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`aoe serve at ${baseUrl} not ready: ${lastErr}`);
}

function writeFakeClaudeShim(binDir: string): void {
  // Dashboard tracer specs only need the tmux pane to stay open with a
  // long-running process. Cockpit specs swap this for the ACP agent shim
  // via `writeFakeAcpShim`.
  const script = "#!/bin/bash\nexec tail -f /dev/null\n";
  const path = join(binDir, "claude");
  writeFileSync(path, script);
  chmodSync(path, 0o755);
}

function writeFakeAcpShim(binDir: string, fakeAcpScript: string | undefined): void {
  // The cockpit supervisor calls `claude` (or `aoe-agent`) and handshakes
  // ACP over its stdio. The shim is a tiny bash wrapper that execs the
  // fake ACP agent under node with the optional script env baked in.
  const fakeAgentJs = resolve(__dirname, "fakeAcpAgent.mjs");
  const scriptLine = fakeAcpScript
    ? `export FAKE_ACP_SCRIPT=${JSON.stringify(fakeAcpScript)}\n`
    : "";
  const script = `#!/bin/bash\n${scriptLine}exec node ${JSON.stringify(fakeAgentJs)} "$@"\n`;
  for (const name of ["claude", "aoe-agent"]) {
    const path = join(binDir, name);
    writeFileSync(path, script);
    chmodSync(path, 0o755);
  }
}

async function enableCockpitMaster(
  baseUrl: string,
  sessionCookie?: { name: string; value: string },
): Promise<void> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (sessionCookie) {
    // Required when authMode === "passphrase"; loopback bypass kicks in
    // for token+loopback callers, but PATCH /api/cockpit/master predates
    // any browser navigation, so the SPA hasn't yet seeded the cookie
    // into a Playwright context. Include it on the harness's own request.
    headers.Cookie = `${sessionCookie.name}=${sessionCookie.value}`;
  }
  const res = await fetch(`${baseUrl}/api/cockpit/master`, {
    method: "PATCH",
    headers,
    body: JSON.stringify({ enabled: true }),
  });
  if (!res.ok) {
    throw new Error(
      `PATCH /api/cockpit/master failed: ${res.status} ${await res.text()}`,
    );
  }
}

async function loginWithPassphrase(
  baseUrl: string,
  passphrase: string,
  deviceBindingSecret: string,
): Promise<{ cookie: { name: string; value: string } }> {
  const res = await fetch(`${baseUrl}/api/login`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ passphrase, device_binding_secret: deviceBindingSecret }),
  });
  if (!res.ok) {
    throw new Error(`POST /api/login failed: ${res.status} ${await res.text()}`);
  }
  const setCookie = res.headers.get("set-cookie") ?? "";
  // axum returns a single Set-Cookie; cookie name we want is "aoe_session".
  const match = /aoe_session=([^;]+)/.exec(setCookie);
  if (!match) {
    throw new Error(`POST /api/login did not set aoe_session cookie. Set-Cookie was: ${setCookie}`);
  }
  return { cookie: { name: "aoe_session", value: match[1] } };
}

export async function spawnAoeServe(opts: SpawnOptions): Promise<ServeHandle> {
  const aoeBinary = resolveAoeBinary();
  if (!existsSync(aoeBinary)) {
    throw new Error(
      `aoe binary not found at ${aoeBinary}. ` +
        `Set AOE_E2E_BINARY or run liveGlobalSetup.ts to build it.`,
    );
  }

  // realpathSync resolves any symlinks in the tmpdir path (on macOS,
  // `/var/folders/...` lives under `/private/var/...`). The server's
  // `/api/filesystem/browse` endpoint canonicalizes the requested path
  // and checks `starts_with(dirs::home_dir())`; if HOME is the un-
  // canonicalized form, that check fails on macOS and any browse call
  // against the test's HOME tree returns "outside the home directory".
  const home = realpathSync(
    mkdtempSync(join(tmpdir(), `aoe-pw-w${opts.workerIndex}-p${opts.parallelIndex}-`)),
  );
  const xdg = join(home, "config");
  const tmp = join(home, "tmp");
  const tmuxTmp = join(home, "tmux");
  const shimBin = join(home, "bin");
  for (const dir of [xdg, tmp, tmuxTmp, shimBin]) {
    mkdirSync(dir, { recursive: true, mode: 0o700 });
  }
  if (opts.cockpit) {
    writeFakeAcpShim(shimBin, opts.fakeAcpScript);
  } else {
    writeFakeClaudeShim(shimBin);
  }

  const seedEnv = {
    ...process.env,
    HOME: home,
    XDG_CONFIG_HOME: xdg,
    TMPDIR: tmp,
    TMUX_TMPDIR: tmuxTmp,
    PATH: `${shimBin}:${process.env.PATH ?? ""}`,
  };

  if (opts.seedFn) {
    await opts.seedFn({ home, shimBin, xdg, tmp, tmuxTmp, env: seedEnv });
  }

  const authMode: AuthMode = opts.authMode ?? "none";
  const passphrase = authMode === "passphrase" ? opts.passphrase ?? DEFAULT_PASSPHRASE : undefined;

  const spawnTimeoutMs = opts.spawnTimeoutMs ?? 10_000;
  let proc: ChildProcess | null = null;
  let port = 0;
  let baseUrl = "";

  for (let attempt = 0; attempt < 5; attempt++) {
    port = portFor(opts.workerIndex, opts.parallelIndex, attempt);
    baseUrl = `http://127.0.0.1:${port}`;
    const args = ["serve", "--host", "127.0.0.1", "--port", String(port)];
    if (authMode === "none") args.push("--no-auth");
    if (passphrase) args.push("--passphrase", passphrase);
    if (opts.readOnly) args.push("--read-only");
    if (opts.extraArgs) args.push(...opts.extraArgs);

    proc = spawn(aoeBinary, args, {
      stdio: ["ignore", "pipe", "pipe"],
      env: seedEnv,
    });

    if (process.env.AOE_E2E_DEBUG === "1") {
      // Append per-worker server stdio + spawn env to a fixed path so
      // CI runs can post-mortem failures without holding open pipes
      // that buffer-fill and stall the harness.
      const logPath = `/tmp/aoe-e2e-debug-${opts.workerIndex}-${opts.parallelIndex}.log`;
      const fs = await import("node:fs");
      const log = fs.createWriteStream(logPath, { flags: "a" });
      log.write(`\n=== spawn ${args.join(" ")} (home=${home}) ===\n`);
      proc.stdout?.on("data", (b) => log.write(`[stdout] ${b}`));
      proc.stderr?.on("data", (b) => log.write(`[stderr] ${b}`));
    }

    let spawnFailed = false;
    proc.once("error", () => {
      spawnFailed = true;
    });

    try {
      await waitForServer(baseUrl, spawnTimeoutMs, proc, authMode);
      break;
    } catch (err) {
      try {
        proc.kill("SIGKILL");
      } catch {
        // ignore
      }
      proc = null;
      if (spawnFailed || attempt === 4) {
        rmSync(home, { recursive: true, force: true });
        throw err;
      }
      // try next port
    }
  }

  if (!proc) {
    rmSync(home, { recursive: true, force: true });
    throw new Error("aoe serve failed to bind on every attempted port");
  }

  const handle: ServeHandle = {
    baseUrl,
    port,
    home,
    shimBin,
    proc,
    authMode,
    passphrase,
    tmuxPrefix: tmuxPrefixFor(aoeBinary),
    async stop() {
      try {
        if (proc && proc.exitCode === null && proc.signalCode === null) {
          proc.kill("SIGTERM");
          // Give the server 2s to drain, then SIGKILL.
          await new Promise<void>((resolveExit) => {
            const t = setTimeout(() => {
              try {
                proc!.kill("SIGKILL");
              } catch {
                // ignore
              }
              resolveExit();
            }, 2000);
            proc!.once("exit", () => {
              clearTimeout(t);
              resolveExit();
            });
          });
        }
      } finally {
        // Best-effort: kill any tmux server bound to the isolated socket
        // before deleting the dir. Cockpit specs leave tmux child
        // processes around that hold open file descriptors and trip
        // ENOTEMPTY on rmSync if not cleaned up first.
        try {
          spawnSync("tmux", ["kill-server"], {
            env: {
              ...process.env,
              HOME: home,
              TMUX_TMPDIR: join(home, "tmux"),
            },
            stdio: "ignore",
          });
        } catch {
          // tmux not installed or no server running; either way we don't
          // care.
        }
        // Removing the home dir wipes the isolated TMUX_TMPDIR socket too.
        // Wrap in try/catch: stale fds, slow umount, or AFS-style retry
        // semantics can leave non-empty dirs that don't matter for the
        // test result.
        try {
          rmSync(home, { recursive: true, force: true });
        } catch {
          // best effort
        }
      }
    },
  };

  if (authMode === "passphrase" && passphrase) {
    const deviceBindingSecret = randomBytes(32).toString("base64url");
    const { cookie } = await loginWithPassphrase(baseUrl, passphrase, deviceBindingSecret);
    handle.sessionCookie = cookie;
    handle.deviceBindingSecret = deviceBindingSecret;
  }

  if (opts.cockpit) {
    await enableCockpitMaster(baseUrl, handle.sessionCookie);
  }

  return handle;
}
