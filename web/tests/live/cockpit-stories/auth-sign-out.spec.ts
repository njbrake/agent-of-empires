// User story: a logged-in user can sign out via the topbar overflow
// menu. After clicking, the LoginPage re-appears.
//
// The "Sign out" entry only renders when `loginRequired` is true on the
// TopBar (TopBar.tsx:51), so this story runs against `servePassphrase`
// with a pre-authed cookie via `seedAuth`. Pure no-auth dashboards
// never expose the menu item.

import { test, expect } from "../../helpers/liveTest";
import { seedAuth } from "../../helpers/liveTest";

test("topbar overflow menu signs the user out and returns to LoginPage", async ({
  servePassphrase,
  page,
}) => {
  await seedAuth(page, servePassphrase);
  await page.goto(servePassphrase.baseUrl);

  await expect(
    page.getByRole("button", { name: "Go to dashboard" }),
  ).toBeVisible({ timeout: 10_000 });

  await page.getByRole("button", { name: "More options" }).click();
  await page.getByRole("menuitem", { name: "Sign out" }).click();

  await expect(page.locator("input#passphrase")).toBeVisible({
    timeout: 10_000,
  });
});
