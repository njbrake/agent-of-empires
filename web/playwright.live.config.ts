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
  // Two workers from day one. Port allocation uses (workerIndex, parallelIndex)
  // and tmux is isolated via TMUX_TMPDIR inside each test's HOME, so workers
  // do not collide. See `tests/helpers/aoeServe.ts`.
  fullyParallel: true,
  workers: 2,
  retries: process.env.CI ? 1 : 0,
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
