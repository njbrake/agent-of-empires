// Live-backend theme switching coverage. Spins up a real `aoe serve`
// process against an isolated $HOME and drives the dashboard's theme
// picker through actual /api/themes/:name + /api/theme/current calls.
// This is the test that would have caught the OnceLock-via-load_theme
// deadlock that the mock-API tests in theme-switch.spec.ts couldn't:
// mocks bypass the resolver entirely.

import { test, expect, type Page } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const AOE_BINARY = resolve(process.cwd(), "..", "target", "debug", "aoe");
const HAS_BINARY = existsSync(AOE_BINARY);

const HARNESS_PORT = Number(process.env.AOE_LIVE_TEST_PORT ?? 18099);
const HARNESS_BASE = `http://127.0.0.1:${HARNESS_PORT}`;
// Builtin pinned to a value that visibly differs from Empire so the
// dashboard repaint is observable.
const SWITCH_TO = "dracula";

interface Daemon {
  proc: ChildProcess;
  home: string;
  cleanup: () => void;
}

async function startServer(): Promise<Daemon> {
  const home = mkdtempSync(join(tmpdir(), "aoe-theme-e2e-"));
  // Foreground (no daemonize), no auth, no tunnel; keeps Ctrl-C
  // semantics on Playwright shutdown.
  const proc = spawn(
    AOE_BINARY,
    [
      "serve",
      "--no-auth",
      "--port",
      String(HARNESS_PORT),
      "--host",
      "127.0.0.1",
    ],
    {
      cwd: process.cwd(),
      env: {
        ...process.env,
        HOME: home,
        // Suppress the dev-build prefix dance; let `aoe serve`'s
        // env-driven app-dir routing pick its own dev path.
        RUST_LOG: "warn",
      },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );

  // Wait until the server is listening (poll /api/themes which has no
  // auth and no side effects).
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    try {
      const res = await fetch(`${HARNESS_BASE}/api/themes`);
      if (res.ok) break;
    } catch {
      // socket not up yet
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  if (Date.now() >= deadline) {
    proc.kill("SIGKILL");
    throw new Error("aoe serve did not start within 20s");
  }

  return {
    proc,
    home,
    cleanup: () => {
      try {
        proc.kill("SIGTERM");
      } catch {
        // already exited
      }
      try {
        rmSync(home, { recursive: true, force: true });
      } catch {
        // best effort
      }
    },
  };
}

async function readCssVar(page: Page, name: string): Promise<string> {
  return await page.evaluate(
    (n) => document.documentElement.style.getPropertyValue(n).trim(),
    name,
  );
}

test.describe("Live aoe serve theme switching (#1189)", () => {
  // Skip gracefully if the Rust binary hasn't been built. CI runs
  // `cargo build --features serve` first; local runs without the
  // binary skip these instead of failing the rest of the suite.
  test.skip(
    !HAS_BINARY,
    `${AOE_BINARY} missing; run \`cargo build --features serve\` first`,
  );

  let daemon: Daemon;

  test.beforeAll(async () => {
    daemon = await startServer();
  });

  test.afterAll(() => {
    daemon?.cleanup();
  });

  test("GET /api/themes/:name returns within 2s and is not stuck in resolve", async () => {
    // The OnceLock-via-load_theme deadlock made this endpoint hang
    // forever per request. AbortController with a tight budget
    // catches that regression cheaply.
    const ctrl = new AbortController();
    const watchdog = setTimeout(() => ctrl.abort(), 2_000);
    try {
      const res = await fetch(`${HARNESS_BASE}/api/themes/${SWITCH_TO}`, {
        signal: ctrl.signal,
      });
      expect(res.ok).toBe(true);
      const body = await res.json();
      expect(body.name).toBe(SWITCH_TO);
      expect(body.source).toBe("builtin");
      expect(body.appearance).toBe("dark");
      expect(body.web.cssVars["--color-surface-900"]).toBe("#282a36");
      expect(body.terminal.cssVars["--term-bg"]).toBe("#282a36");
      expect(body.syntax.shikiTheme).toBe("dracula");
    } finally {
      clearTimeout(watchdog);
    }
  });

  test("GET /api/themes/:name handles all 6 builtins sequentially without hanging", async () => {
    const builtins = [
      "empire",
      "phosphor",
      "tokyo-night-storm",
      "catppuccin-latte",
      "dracula",
      "rose-pine",
    ];
    for (const name of builtins) {
      const ctrl = new AbortController();
      const watchdog = setTimeout(() => ctrl.abort(), 2_000);
      try {
        const res = await fetch(`${HARNESS_BASE}/api/themes/${name}`, {
          signal: ctrl.signal,
        });
        expect(res.ok, `${name} did not return`).toBe(true);
        const body = await res.json();
        expect(body.name).toBe(name);
        expect(body.web.cssVars).toBeTruthy();
      } finally {
        clearTimeout(watchdog);
      }
    }
  });

  test("dashboard chrome repaints when theme switches via API", async ({
    page,
  }) => {
    await page.goto(`${HARNESS_BASE}/`);
    // Wait for the React-side fetch of /api/theme/current to land
    // and seed --color-surface-900 with the active palette.
    await expect
      .poll(
        async () => {
          const v = await readCssVar(page, "--color-surface-900");
          return v.length > 0;
        },
        { timeout: 10_000, intervals: [100, 250, 500] },
      )
      .toBe(true);

    // Drive a theme switch by patching the profile-scoped settings
    // endpoint the picker uses, then firing the same event the
    // ThemeSettings.tsx save() handler dispatches.
    const patch = await page.request.patch(
      `${HARNESS_BASE}/api/profiles/default/settings`,
      {
        data: { theme: { name: SWITCH_TO } },
      },
    );
    expect(patch.ok()).toBe(true);

    await page.evaluate((name) => {
      window.dispatchEvent(
        new CustomEvent("aoe:theme-picker-changed", {
          detail: { name },
        }),
      );
    }, SWITCH_TO);

    await expect
      .poll(() => readCssVar(page, "--color-surface-900"), {
        timeout: 5_000,
        intervals: [100, 200, 400],
      })
      .toBe("#282a36");

    const bg = await page.evaluate(
      () => getComputedStyle(document.body).backgroundColor,
    );
    expect(bg).toMatch(/40,\s*42,\s*54/);
  });
});
