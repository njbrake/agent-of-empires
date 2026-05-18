// Playwright fixtures for live-backend tests.
//
// Three serve fixtures map onto the three harness modes:
//   - `serve`          : no-auth, dashboard golden path.
//   - `servePassphrase`: passphrase mode; specs call seedAuth(page, ...)
//                        before navigating.
//   - `serveReadOnly`  : --read-only flag set on aoe serve.
//
// The `page` fixture is wrapped so that when `AOE_COVERAGE=1` is set, the
// `window.__coverage__` object emitted by `vite-plugin-istanbul` is read
// once per test and written as JSON under `web/coverage/playwright/`.
// `web/scripts/merge-coverage.mjs` picks those JSONs up.
//
// Specs do:
//
//   import { test, expect, seedAuth } from "../helpers/liveTest";
//
//   test("dashboard loads", async ({ serve, page }) => {
//     await page.goto(serve.baseUrl);
//     ...
//   });

import { test as base, expect, type Page } from "@playwright/test";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import { spawnAoeServe, type ServeHandle } from "./aoeServe";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const playwrightCoverageDir = resolve(
  __dirname,
  "..",
  "..",
  "coverage",
  "playwright",
);

type LiveFixtures = {
  serve: ServeHandle;
  servePassphrase: ServeHandle;
  serveReadOnly: ServeHandle;
};

async function captureCoverage(page: Page, testTitle: string): Promise<void> {
  if (process.env.AOE_COVERAGE !== "1") return;
  try {
    const coverage = await page.evaluate(
      () => (window as unknown as { __coverage__?: unknown }).__coverage__,
    );
    if (!coverage) return;
    await mkdir(playwrightCoverageDir, { recursive: true });
    const safe = testTitle
      .replace(/[^a-zA-Z0-9_-]/g, "_")
      .slice(0, 160);
    const filename = `${safe}-${randomUUID()}.json`;
    await writeFile(
      resolve(playwrightCoverageDir, filename),
      JSON.stringify(coverage),
    );
  } catch {
    // Test may have closed the page or navigated cross-origin. Coverage
    // gaps in those edge cases are acceptable; we'd rather not fail the
    // test because of a coverage-collection hiccup.
  }
}

/**
 * Seed the session cookie + device binding secret onto a page so a
 * passphrase-mode `aoe serve` accepts subsequent navigations. Call
 * before `page.goto(handle.baseUrl)`.
 */
export async function seedAuth(
  page: Page,
  handle: ServeHandle,
): Promise<void> {
  if (!handle.sessionCookie) return;
  const url = new URL(handle.baseUrl);
  await page.context().addCookies([
    {
      name: handle.sessionCookie.name,
      value: handle.sessionCookie.value,
      domain: url.hostname,
      path: "/",
      httpOnly: true,
      sameSite: "Strict",
    },
  ]);
  if (handle.deviceBindingSecret) {
    const secret = handle.deviceBindingSecret;
    await page.addInitScript((s) => {
      try {
        window.localStorage.setItem("aoe-device-binding-secret", s);
      } catch {
        // localStorage may be unavailable depending on origin state.
      }
    }, secret);
  }
}

export const test = base.extend<LiveFixtures>({
  serve: async ({}, use, testInfo) => {
    const h = await spawnAoeServe({
      authMode: "none",
      workerIndex: testInfo.workerIndex,
      parallelIndex: testInfo.parallelIndex,
    });
    await use(h);
    await h.stop();
  },
  servePassphrase: async ({}, use, testInfo) => {
    const h = await spawnAoeServe({
      authMode: "passphrase",
      workerIndex: testInfo.workerIndex,
      parallelIndex: testInfo.parallelIndex,
    });
    await use(h);
    await h.stop();
  },
  serveReadOnly: async ({}, use, testInfo) => {
    const h = await spawnAoeServe({
      authMode: "none",
      readOnly: true,
      workerIndex: testInfo.workerIndex,
      parallelIndex: testInfo.parallelIndex,
    });
    await use(h);
    await h.stop();
  },
  page: async ({ page }, use, testInfo) => {
    await use(page);
    await captureCoverage(page, testInfo.titlePath.join(" > "));
  },
});

export { expect };
export type { ServeHandle };
