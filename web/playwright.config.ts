import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  // Live-backend specs (spawn real `aoe serve`) live under tests/live/ and
  // run via playwright.live.config.ts.
  testIgnore: ["**/live/**"],
  timeout: 30000,
  retries: process.env.CI ? 1 : 0,
  // Mocked specs share one `vite preview` server and otherwise touch no
  // shared state, so they're safe to run fully in parallel. Without these
  // settings Playwright defaults to half-CPU workers (2 on the 4-vCPU
  // ubuntu-latest runner) and file-level parallelism only, which was
  // pinning the mocked job at ~5 min vs the live job's ~3 min.
  fullyParallel: true,
  workers: process.env.CI ? 4 : undefined,
  use: {
    baseURL: "http://localhost:4173",
    headless: true,
    screenshot: "only-on-failure",
  },
  webServer: {
    command: "npx vite preview --port 4173",
    port: 4173,
    reuseExistingServer: !process.env.CI,
  },
  reporter: process.env.CI ? [["html", { open: "never" }], ["github"]] : "list",
  projects: [
    {
      name: "chromium",
      use: { browserName: "chromium" },
    },
  ],
});
