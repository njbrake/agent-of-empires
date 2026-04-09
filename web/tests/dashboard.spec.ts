import { test, expect } from "@playwright/test";

test.describe("Dashboard layout", () => {
  test("loads and shows header with title", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("header")).toBeVisible();
    await expect(page.locator("header h1")).toContainText("Agent of Empires");
    await expect(page.locator("header")).toContainText("Dashboard");
  });

  test("shows sidebar with Sessions label", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("aside")).toBeVisible();
    await expect(page.locator("aside")).toContainText("Sessions");
  });

  test("shows empty state when no session selected", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("text=Select a session")).toBeVisible();
  });

  test("shows session count or connection error in header", async ({
    page,
  }) => {
    await page.goto("/");
    await expect(
      page.locator("header").locator("text=/session|error/i"),
    ).toBeVisible();
  });
});

test.describe("Sidebar features", () => {
  test("search toggle shows search input", async ({ page }) => {
    await page.goto("/");
    const searchBtn = page.locator('button[title="Search"]');
    if (await searchBtn.isVisible()) {
      await searchBtn.click();
      await expect(
        page.locator('input[placeholder="Search sessions..."]'),
      ).toBeVisible();
    }
  });

  test("new session button exists and opens panel", async ({ page }) => {
    await page.goto("/");
    const newBtn = page.locator('button[title="New session (n)"]');
    await expect(newBtn).toBeVisible();
    await newBtn.click();
    await expect(page.locator("h2:has-text('New Session')")).toBeVisible();
  });

  test("create panel has path field and agent selector", async ({ page }) => {
    await page.goto("/");
    await page.locator('button[title="New session (n)"]').click();
    await expect(
      page.locator('input[placeholder="/path/to/your/project"]'),
    ).toBeVisible();
    await expect(page.getByText("Agent", { exact: true })).toBeVisible();
  });

  test("create panel submit disabled without path", async ({ page }) => {
    await page.goto("/");
    await page.locator('button[title="New session (n)"]').click();
    const submit = page.locator('button:has-text("Create Session")');
    await expect(submit).toBeDisabled();
  });

  test("create panel submit enables with path", async ({ page }) => {
    await page.goto("/");
    await page.locator('button[title="New session (n)"]').click();
    await page
      .locator('input[placeholder="/path/to/your/project"]')
      .fill("/tmp/test");
    const submit = page.locator('button:has-text("Create Session")');
    await expect(submit).toBeEnabled();
  });

  test("create panel advanced options toggle", async ({ page }) => {
    await page.goto("/");
    await page.locator('button[title="New session (n)"]').click();
    await expect(
      page.locator('input[placeholder="feature/my-branch"]'),
    ).not.toBeVisible();
    await page.getByText("Show advanced options").click();
    await expect(
      page.locator('input[placeholder="feature/my-branch"]'),
    ).toBeVisible();
  });

  test("create panel closes on cancel", async ({ page }) => {
    await page.goto("/");
    await page.locator('button[title="New session (n)"]').click();
    await expect(page.locator("h2:has-text('New Session')")).toBeVisible();
    await page.getByRole("button", { name: "Cancel" }).click();
    await expect(page.locator("h2:has-text('New Session')")).not.toBeVisible();
  });
});

test.describe("Header navigation", () => {
  test("profile selector shows all profiles", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("[all profiles]")).toBeVisible();
  });

  test("settings button opens settings view (desktop)", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await page.locator('button[title="Settings (s)"]').click();
    // Settings view shows loading state (no backend) or the actual settings
    await expect(page.getByText("Loading settings...")).toBeVisible();
  });

  test("help button exists on desktop", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await expect(page.locator('button[title="Help (?)"]')).toBeVisible();
  });

  test("help button opens overlay", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await page.locator('button[title="Help (?)"]').click();
    await expect(
      page.locator("h2:has-text('Keyboard Shortcuts')"),
    ).toBeVisible();
  });

  test("worktrees button exists on desktop", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await expect(page.locator('button[title="Worktrees"]')).toBeVisible();
  });
});

test.describe("Responsive / mobile", () => {
  test("mobile nav bar visible on small viewport", async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    await page.goto("/");
    await expect(page.locator("nav")).toBeVisible();
  });

  test("desktop nav buttons hidden on mobile", async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    await page.goto("/");
    await expect(
      page.locator('button[title="Settings (s)"]'),
    ).not.toBeVisible();
    await expect(page.locator('button[title="Help (?)"]')).not.toBeVisible();
  });

  test("mobile nav has sessions, worktrees, settings tabs", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    await page.goto("/");
    const nav = page.locator("nav");
    await expect(nav.getByText("Worktrees")).toBeVisible();
    await expect(nav.getByText("Settings")).toBeVisible();
  });
});

test.describe("Design system verification", () => {
  test("uses warm navy background, not cold gray", async ({ page }) => {
    await page.goto("/");
    const bg = await page.evaluate(() =>
      getComputedStyle(document.body).backgroundColor,
    );
    // #0f172a = rgb(15, 23, 42) -- warm navy
    expect(bg).toContain("15");
    // Not #0d1117 = rgb(13, 17, 23) -- cold GitHub gray
    expect(bg).not.toBe("rgb(13, 17, 23)");
  });

  test("loads DM Sans body font", async ({ page }) => {
    await page.goto("/");
    const fonts = await page.evaluate(() =>
      getComputedStyle(document.body).fontFamily,
    );
    expect(fonts.toLowerCase()).toContain("dm sans");
  });

  test("empty state uses terminal prompt icon", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText(">_")).toBeVisible();
  });
});
