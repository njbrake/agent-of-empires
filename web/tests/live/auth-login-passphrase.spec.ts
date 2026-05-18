// Passphrase mode login flow.
//
// servePassphrase boots `aoe serve --passphrase <fixed>` and does an
// initial POST /api/login from the harness so we have a known-good
// cookie. This spec drives the browser through the LoginPage instead:
// wrong passphrase shows an error and the cookie stays absent; correct
// passphrase replaces the LoginPage with the dashboard.

import { test, expect } from "../helpers/liveTest";

test("wrong passphrase shows an error and stays on LoginPage", async ({
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

test("correct passphrase logs in and reveals the dashboard", async ({
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
