import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  // Live-backend specs (spawn real `aoe serve`) live under tests/live/ and
  // run via playwright.live.config.ts.
  testIgnore: ["**/live/**"],
  timeout: 30000,
  retries: process.env.CI ? 1 : 0,
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
