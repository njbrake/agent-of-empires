import { test, expect } from "@playwright/test";

// Verifies URL-based routing: deep links land on the right view, refresh
// preserves location, and back/forward replays history.
test.describe("URL routing", () => {
  test("'/' renders the dashboard home screen", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Open project")).toBeVisible();
    await expect(page).toHaveURL("/");
  });

  test("'/settings' renders settings on first load", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("Settings", { exact: true }).first()).toBeVisible();
    await expect(page).toHaveURL("/settings");
  });

  test("settings tab is reflected in the URL", async ({ page }) => {
    await page.goto("/settings/theme");
    await expect(page.getByRole("heading", { name: "Theme" })).toBeVisible();
    await expect(page).toHaveURL("/settings/theme");
  });

  test("refresh on /settings keeps user on settings", async ({ page }) => {
    await page.goto("/settings");
    await expect(page.getByText("Settings", { exact: true }).first()).toBeVisible();
    await page.reload();
    await expect(page.getByText("Settings", { exact: true }).first()).toBeVisible();
    await expect(page).toHaveURL("/settings");
  });

  test("'/session/<id>' for an unknown session falls back to dashboard", async ({ page }) => {
    // No backend, sessions list is empty, so the route still matches but
    // the resolver finds no session and the dashboard renders. Importantly
    // the URL stays put so a real backend can later resolve it.
    await page.goto("/session/does-not-exist");
    await expect(page.getByText("Open project")).toBeVisible();
    await expect(page).toHaveURL("/session/does-not-exist");
  });

  test("legacy '?session=X' URL is rewritten to '/session/X'", async ({ page }) => {
    await page.goto("/?session=abc-123");
    await expect(page).toHaveURL("/session/abc-123");
  });

  test("browser back navigates dashboard ↔ settings", async ({ page }) => {
    await page.goto("/");
    await page.goto("/settings");
    await expect(page).toHaveURL("/settings");
    await page.goBack();
    await expect(page).toHaveURL("/");
    await page.goForward();
    await expect(page).toHaveURL("/settings");
  });
});
