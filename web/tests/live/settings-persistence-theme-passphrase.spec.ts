// Theme persistence under `--auth=passphrase` (#1510).
//
// Story 2: a passphrase user picks a theme. The dashboard repaints,
// the new theme survives a page reload AND an `aoe serve` restart,
// and NO passphrase re-prompt fires. Locks the body-shape elevation
// gate in `update_profile_settings`: theme is on the safe list, so
// the handler must not return 403 elevation_required and the client
// must not pop ElevationPrompt.
//
// Story 3: the same passphrase user PATCHes a sandbox image. The
// daemon DOES still return 403 elevation_required and the client
// DOES pop the inline passphrase prompt. Locks the threat-model
// half of the fix: tamper-surface fields stay gated.
//
// Both tests boot a fresh `aoe serve --auth=passphrase` via
// `spawnAoeServe({ preloginViaHarness: true })` and inject the
// resulting session cookie + device binding so the browser starts
// authenticated but NOT elevated. Elevation is the second factor
// the issue is about.

import { test as base, expect, type Page } from "@playwright/test";
import {
  spawnAoeServe,
  type ServeHandle,
} from "../helpers/aoeServe";
import { seedAuth } from "../helpers/liveTest";

const SWITCH_TO = "dracula";

const test = base.extend<{ servePreauthed: ServeHandle }>({
  servePreauthed: async ({}, use, testInfo) => {
    const handle = await spawnAoeServe({
      authMode: "passphrase",
      workerIndex: testInfo.workerIndex,
      parallelIndex: testInfo.parallelIndex,
      preloginViaHarness: true,
    });
    await use(handle);
    await handle.stop();
  },
});

async function gotoPreauthed(page: Page, handle: ServeHandle, path: string) {
  await seedAuth(page, handle);
  await page.goto(`${handle.baseUrl}${path}`);
}

test("theme picker persists across reload + restart without passphrase prompt", async ({
  servePreauthed,
  page,
}) => {
  await gotoPreauthed(page, servePreauthed, "/settings/theme");

  const themeSelect = page
    .locator("label", { hasText: /^Theme$/ })
    .locator("..")
    .locator("select");
  await expect(themeSelect).toBeVisible({ timeout: 10_000 });
  await expect
    .poll(
      async () =>
        await themeSelect.evaluate((sel: HTMLSelectElement, target) =>
          Array.from(sel.options).some((o) => o.value === target),
        SWITCH_TO),
      { timeout: 5_000 },
    )
    .toBe(true);

  // Listen for the elevation prompt event the fetchInterceptor fires
  // on 403 elevation_required, so a flaky "dialog never opened" race
  // can't silently let the test pass.
  const elevationFired = await page.evaluate(() => {
    (window as unknown as { __elevationFired?: boolean }).__elevationFired = false;
    window.addEventListener("aoe:elevation-required", () => {
      (window as unknown as { __elevationFired?: boolean }).__elevationFired = true;
    });
    return true;
  });
  expect(elevationFired).toBe(true);

  await themeSelect.selectOption(SWITCH_TO);

  await expect(async () => {
    const after = await fetch(`${servePreauthed.baseUrl}/api/profiles/default/settings`, {
      headers: cookieHeader(servePreauthed),
    }).then((r) => r.json());
    expect(after?.theme?.name).toBe(SWITCH_TO);
  }).toPass({ timeout: 5_000 });

  // No elevation prompt fired anywhere. Checks both the DOM dialog
  // and the event the interceptor would have dispatched.
  await expect(
    page.getByRole("dialog", { name: /Confirm passphrase/i }),
  ).toHaveCount(0);
  const fired = await page.evaluate(
    () =>
      (window as unknown as { __elevationFired?: boolean }).__elevationFired ??
      false,
  );
  expect(fired).toBe(false);

  // Client-side repaint after PATCH resolves.
  await expect
    .poll(
      () =>
        page.evaluate(() =>
          document.documentElement.style
            .getPropertyValue("--color-surface-900")
            .trim(),
        ),
      { timeout: 5_000, intervals: [100, 200, 400] },
    )
    .toBe("#282a36");

  await page.reload();
  const afterReload = await fetch(
    `${servePreauthed.baseUrl}/api/profiles/default/settings`,
    { headers: cookieHeader(servePreauthed) },
  ).then((r) => r.json());
  expect(afterReload?.theme?.name).toBe(SWITCH_TO);

  await servePreauthed.restart();
  const afterRestart = await fetch(
    `${servePreauthed.baseUrl}/api/profiles/default/settings`,
    { headers: cookieHeader(servePreauthed) },
  ).then((r) => r.json());
  expect(afterRestart?.theme?.name).toBe(SWITCH_TO);
});

test("sandbox image change still requires passphrase elevation", async ({
  servePreauthed,
  page,
}) => {
  await gotoPreauthed(page, servePreauthed, "/");

  // Fire the PATCH from the page so the fetchInterceptor (installed by
  // the SPA bootstrap) sees the 403 and dispatches the elevation event.
  // A direct test-side `fetch` would bypass the interceptor and the
  // dialog would never open even when the server side is correct.
  const status = await page.evaluate(async () => {
    const res = await fetch("/api/profiles/default/settings", {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        sandbox: { default_image: "ghcr.io/example/img:tampered" },
      }),
    });
    return res.status;
  });
  expect(status).toBe(403);

  // ElevationPrompt opens (from fetchInterceptor dispatching
  // ELEVATION_REQUIRED_EVENT on the 403 elevation_required payload).
  await expect(
    page.getByRole("dialog", { name: /Confirm passphrase/i }),
  ).toBeVisible({ timeout: 5_000 });

  // Server state did not move: the sandbox image stays at whatever it
  // was. Use the seeded session cookie on the test-side fetch so the
  // GET passes the passphrase wall.
  const after = await fetch(
    `${servePreauthed.baseUrl}/api/profiles/default/settings`,
    { headers: cookieHeader(servePreauthed) },
  ).then((r) => r.json());
  expect(after?.sandbox?.default_image ?? "").not.toBe(
    "ghcr.io/example/img:tampered",
  );
});

function cookieHeader(handle: ServeHandle): Record<string, string> {
  if (!handle.sessionCookie) return {};
  return {
    Cookie: `${handle.sessionCookie.name}=${handle.sessionCookie.value}`,
    "X-Aoe-Device-Binding": handle.deviceBindingSecret ?? "",
  };
}
