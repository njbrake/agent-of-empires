// Live-backend Playwright config.
//
// Live specs under `tests/live/` spawn a real `aoe serve` per test via
// `tests/helpers/aoeServe.ts`, with isolated HOME/XDG/TMPDIR/TMUX_TMPDIR
// and a worker-indexed port range. There is no global Vite preview server;
// each test gets its own backend.
//
// Use `npx playwright test --config=playwright.live.config.ts`.
//
// CI runs this from the `playwright-live` job (see .github/workflows/ci.yml)
// after building `aoe` with `--features serve --release`. Local runs pick up
// the binary from `AOE_E2E_BINARY` or fall back to `../target/release/aoe`.

import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/live",
  globalSetup: "./tests/helpers/liveGlobalSetup.ts",
  // Live specs do real I/O (cargo binary spawn, tmux, fetch). Give them more
  // headroom than the mocked suite's 30s.
  timeout: 60_000,
  // Six workers on the 4-core GitHub runner. Each worker spawns its
  // own `aoe serve` against an isolated HOME / TMUX_TMPDIR with a
  // (workerIndex, parallelIndex)-derived port, so workers don't
  // collide. The live suite is I/O-bound (tmux / fetch round-trips),
  // so over-subscribing CPU is fine and keeps the job under the 6-minute
  // wall-time budget as cockpit-stories/ grows.
  fullyParallel: true,
  workers: 6,
  // No retries: a flaky live spec doubles CI wall time on every flake.
  // Better to surface the flake and fix it (or quarantine via test.skip
  // until fixed) than to mask it with a retry budget.
  retries: 0,
  use: {
    headless: true,
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  reporter: process.env.CI
    ? [
        ["html", { open: "never", outputFolder: "playwright-live-report" }],
        ["github"],
      ]
    : "list",
  projects: [
    {
      name: "chromium",
      use: { browserName: "chromium" },
    },
  ],
});
