// Passphrase mode login flow.
//
// Both tests are SKIPPED in this PR. Reason: upstream #1190 added a
// loopback-bypass for the passphrase factor — a loopback caller that
// presents a valid bearer token skips LoginPage entirely. The harness
// always runs from 127.0.0.1, so the LoginPage that this spec is
// designed to drive never renders. A non-loopback test environment
// (or an explicit loopback-bypass override) is needed to revive these.
//
// Tracked under #1226 (token-mode auth coverage) alongside the rest of
// the auth surface. The harness still supports `authMode: "passphrase"`
// and `loginWithPassphrase` works correctly for non-loopback callers
// who choose to use it.

import { test, expect } from "../helpers/liveTest";

test.skip("wrong passphrase shows an error and stays on LoginPage", async ({
  servePassphrase,
  page,
}) => {
  await page.goto(servePassphrase.baseUrl);

  await expect(page.locator("input#passphrase")).toBeVisible();
  await page.locator("input#passphrase").fill("definitely-wrong");
  await page.locator("button[type=submit]").click();

  // The login lib surfaces server errors via the LoginPage's error
  // state; the field stays focused for retry.
  await expect(page.locator("input#passphrase")).toBeVisible();
  await expect(page.getByText(/(invalid|wrong|fail)/i)).toBeVisible({
    timeout: 5_000,
  });
});

test.skip("correct passphrase logs in and reveals the dashboard", async ({
  servePassphrase,
  page,
}) => {
  if (!servePassphrase.passphrase) {
    throw new Error("servePassphrase fixture must expose passphrase");
  }

  await page.goto(servePassphrase.baseUrl);
  await expect(page.locator("input#passphrase")).toBeVisible();
  await page.locator("input#passphrase").fill(servePassphrase.passphrase);
  await page.locator("button[type=submit]").click();

  // Once login resolves, the LoginPage unmounts and the dashboard's
  // top bar takes over. Wait for any indicator that we left LoginPage:
  // the passphrase field is gone.
  await expect(page.locator("input#passphrase")).toBeHidden({
    timeout: 10_000,
  });
});
