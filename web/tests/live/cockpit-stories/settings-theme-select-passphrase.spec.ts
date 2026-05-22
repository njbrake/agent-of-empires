// User story: change the theme via the settings ThemeSettings select
// on a passphrase-protected server.
//
// Mirror of settings-theme-select but the SettingsView is only
// reachable after driving the LoginPage. Confirms that the
// PATCH /api/settings request carries the session cookie minted by
// /api/login and the theme persists across reload (which re-fetches
// the cookie from the browser jar).

import { test, expect } from "../../helpers/liveTest";
import { openSettingsTab, settingsSelectByLabel } from "../../helpers/cockpit";

test("theme select round-trips through the UI under passphrase auth", async ({
  servePassphrase,
  page,
}) => {
  if (!servePassphrase.passphrase) {
    throw new Error("servePassphrase fixture must expose passphrase");
  }

  await page.goto(servePassphrase.baseUrl);
  await page.locator("input#passphrase").fill(servePassphrase.passphrase);
  await page.locator("button[type=submit]").click();
  await expect(
    page.getByRole("button", { name: "Go to dashboard" }),
  ).toBeVisible({ timeout: 10_000 });

  // Drive the in-app sidebar navigation to /settings rather than
  // page.goto'ing the URL: with passphrase auth, the direct URL load
  // hits the LoginPage briefly and the SPA can land back on the
  // dashboard before SettingsView mounts. Clicking the sidebar
  // affordance keeps the established session cookie + history state.
  await page.getByRole("button", { name: "Settings" }).click();
  await openSettingsTab(page, "Theme");

  const themeSelect = settingsSelectByLabel(page, "Theme");
  await expect(themeSelect).toBeVisible({ timeout: 10_000 });
  await expect
    .poll(async () => themeSelect.locator("option").count(), {
      timeout: 10_000,
    })
    .toBeGreaterThan(0);

  const optionValues = await themeSelect
    .locator("option")
    .evaluateAll((els) => (els as HTMLOptionElement[]).map((o) => o.value));
  const current = await themeSelect.inputValue();
  const next = optionValues.find((v) => v && v !== current);
  expect(
    next,
    "theme select needs at least one option distinct from current",
  ).toBeDefined();

  await themeSelect.selectOption(next!);
  await expect(themeSelect).toHaveValue(next!);

  // Resolved theme must paint via `document.documentElement.dataset.theme`.
  await expect
    .poll(
      async () =>
        await page.evaluate(() => document.documentElement.dataset.theme),
      { timeout: 10_000 },
    )
    .toBe(next);

  await page.reload();
  await openSettingsTab(page, "Theme");
  const reloaded = settingsSelectByLabel(page, "Theme");
  await expect(reloaded).toHaveValue(next!, { timeout: 10_000 });
  // Bootstrap path must also re-apply the theme after reload under
  // passphrase auth (cookie must persist + theme path resolves).
  await expect
    .poll(
      async () =>
        await page.evaluate(() => document.documentElement.dataset.theme),
      { timeout: 10_000 },
    )
    .toBe(next);
});
