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
import { spawnAoeServe, type ServeHandle } from "./aoeServe";
import { captureCoverage } from "./coverageCapture";

type LiveFixtures = {
  serve: ServeHandle;
  servePassphrase: ServeHandle;
  serveReadOnly: ServeHandle;
  /**
   * Cockpit fixture. Only supported with `authMode: "none"` today; the
   * harness calls `PATCH /api/cockpit/master` without a session cookie.
   * Token-mode cockpit coverage is queued in #1226. If you need
   * passphrase + cockpit, call `spawnAoeServe` directly and pass the
   * `sessionCookie` through to the master-enable request.
   */
  serveCockpit: ServeHandle;
};

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
  serveCockpit: async ({}, use, testInfo) => {
    const h = await spawnAoeServe({
      authMode: "none",
      cockpit: true,
      // Specs that want a custom script set `FAKE_ACP_SCRIPT` themselves
      // through testInfo.use() overrides or call `spawnAoeServe` directly.
      fakeAcpScript: process.env.FAKE_ACP_SCRIPT,
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
