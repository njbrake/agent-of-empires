import { test, expect } from "@playwright/test";

test.describe("Top bar", () => {
  test("renders sidebar toggle, brand, palette pill, and overflow", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await expect(page.getByRole("button", { name: "Toggle sidebar" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Go to dashboard" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Open command palette" }).first()).toBeVisible();
    await expect(page.getByRole("button", { name: "More options" })).toBeVisible();
  });

  test("overflow menu opens on click and exposes Settings", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await page.getByRole("button", { name: "More options" }).click();
    await expect(page.getByRole("menuitem", { name: "Settings" })).toBeVisible();
    await expect(page.getByRole("menuitem", { name: "Keyboard shortcuts" })).toBeVisible();
  });

  test("overflow menu closes on outside click", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await page.getByRole("button", { name: "More options" }).click();
    await expect(page.getByRole("menuitem", { name: "Settings" })).toBeVisible();
    await page.mouse.click(300, 300);
    await expect(page.getByRole("menuitem", { name: "Settings" })).not.toBeVisible();
  });

  test("overflow Settings triggers settings view", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await page.getByRole("button", { name: "More options" }).click();
    await page.getByRole("menuitem", { name: "Settings" }).click();
    await expect(page.getByRole("button", { name: /Back/i })).toBeVisible();
  });

  test("overflow Keyboard shortcuts opens help overlay", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await page.getByRole("button", { name: "More options" }).click();
    await page.getByRole("menuitem", { name: "Keyboard shortcuts" }).click();
    await expect(page.getByRole("heading", { name: "Keyboard Shortcuts" })).toBeVisible();
  });

  test("offline indicator shows when API unreachable", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("offline")).toBeVisible();
  });

  test("mobile: palette trigger collapses to icon", async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    await page.goto("/");
    // The icon-only variant is still accessible via the same aria-label
    await expect(page.getByRole("button", { name: "Open command palette" }).first()).toBeVisible();
  });
});
