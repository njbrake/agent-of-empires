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

import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, writeFileSync, chmodSync, mkdirSync, rmSync } from "node:fs";
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
  stop(): Promise<void>;
}

export function resolveAoeBinary(): string {
  const fromEnv = process.env.AOE_E2E_BINARY;
  if (fromEnv && existsSync(fromEnv)) return fromEnv;
  const repoRoot = resolve(__dirname, "..", "..", "..");
  const release = join(repoRoot, "target", "release", "aoe");
  return release;
}

function portFor(workerIndex: number, parallelIndex: number, attempt: number): number {
  // 5200 + worker*100 + parallel + attempt*7 covers ~14 retries per
  // (worker, parallel) slot before colliding with the next slot.
  return 5200 + workerIndex * 100 + parallelIndex + attempt * 7;
}

async function waitForServer(baseUrl: string, deadlineMs: number): Promise<void> {
  const deadline = Date.now() + deadlineMs;
  let lastErr: unknown = "no attempts made";
  while (Date.now() < deadline) {
    try {
      const res = await fetch(`${baseUrl}/api/about`);
      // /api/about returns 401 under passphrase mode (auth required) and
      // 200 under no-auth. Either response shape is proof the HTTP listener
      // is bound.
      if (res.status === 200 || res.status === 401) return;
      lastErr = `status ${res.status}`;
    } catch (err) {
      lastErr = err;
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`aoe serve at ${baseUrl} not ready: ${lastErr}`);
}

function writeFakeClaudeShim(binDir: string): void {
  // The dashboard tracer specs only need the tmux pane to stay open with a
  // long-running process. Cockpit specs (C11) will replace this with a fake
  // ACP agent script via the `cockpit` option.
  const script = "#!/bin/bash\nexec tail -f /dev/null\n";
  const path = join(binDir, "claude");
  writeFileSync(path, script);
  chmodSync(path, 0o755);
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

  const home = mkdtempSync(join(tmpdir(), `aoe-pw-w${opts.workerIndex}-p${opts.parallelIndex}-`));
  const xdg = join(home, "config");
  const tmp = join(home, "tmp");
  const tmuxTmp = join(home, "tmux");
  const shimBin = join(home, "bin");
  for (const dir of [xdg, tmp, tmuxTmp, shimBin]) {
    mkdirSync(dir, { recursive: true, mode: 0o700 });
  }
  writeFakeClaudeShim(shimBin);

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
      env: {
        ...process.env,
        HOME: home,
        XDG_CONFIG_HOME: xdg,
        TMPDIR: tmp,
        TMUX_TMPDIR: tmuxTmp,
        PATH: `${shimBin}:${process.env.PATH ?? ""}`,
      },
    });

    let spawnFailed = false;
    proc.once("error", () => {
      spawnFailed = true;
    });

    try {
      await waitForServer(baseUrl, spawnTimeoutMs);
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
        // Removing the home dir wipes the isolated TMUX_TMPDIR socket too.
        // Orphaned tmux child processes inside the dead socket are inert.
        rmSync(home, { recursive: true, force: true });
      }
    },
  };

  if (authMode === "passphrase" && passphrase) {
    const deviceBindingSecret = randomBytes(32).toString("base64url");
    const { cookie } = await loginWithPassphrase(baseUrl, passphrase, deviceBindingSecret);
    handle.sessionCookie = cookie;
    handle.deviceBindingSecret = deviceBindingSecret;
  }

  return handle;
}
