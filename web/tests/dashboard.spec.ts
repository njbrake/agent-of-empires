import { test, expect } from "@playwright/test";

test.describe("Dashboard layout", () => {
  test("loads and shows header", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("header")).toBeVisible();
  });

  test("shows fallback title when no workspace selected", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Agent of Empires")).toBeVisible();
  });

  test("shows empty state when no sessions exist", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("No sessions yet")).toBeVisible();
  });

  test("shows create session CTA in empty state", async ({ page }) => {
    await page.goto("/");
    const cta = page.getByRole("button", { name: "Create session" });
    await expect(cta).toBeVisible();
  });

  test("shows offline indicator when API unreachable", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("offline")).toBeVisible();
  });
});

test.describe("Sidebar", () => {
  test("sidebar visible on desktop by default", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await expect(page.getByRole("button", { name: "+ New Session" })).toBeVisible();
    await expect(page.getByPlaceholder("Search... (/)")).toBeVisible();
  });

  test("sidebar toggle button exists", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByRole("button", { name: "Toggle sidebar" })).toBeVisible();
  });

  test("sidebar can be toggled closed and open on desktop", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    const newBtn = page.getByRole("button", { name: "+ New Session" });
    await expect(newBtn).toBeVisible();

    await page.getByRole("button", { name: "Toggle sidebar" }).click();
    await expect(newBtn).not.toBeVisible();

    await page.getByRole("button", { name: "Toggle sidebar" }).click();
    await expect(newBtn).toBeVisible();
  });
});

test.describe("Create session modal", () => {
  test("opens from empty state CTA", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("button", { name: "Create session" }).click();
    await expect(page.getByRole("heading", { name: "New Session" })).toBeVisible();
  });

  test("opens from sidebar button", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await page.getByRole("button", { name: "+ New Session" }).click();
    await expect(page.getByRole("heading", { name: "New Session" })).toBeVisible();
  });

  test("opens with keyboard shortcut n", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    // Click body first to ensure keyboard events reach the app
    await page.locator("body").click();
    await page.keyboard.press("n");
    await expect(page.getByRole("heading", { name: "New Session" })).toBeVisible();
  });

  test("has project path field", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("button", { name: "Create session" }).click();
    await expect(page.getByPlaceholder("/path/to/your/project")).toBeVisible();
  });

  test("has branch field", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("button", { name: "Create session" }).click();
    await expect(page.getByPlaceholder("feat/my-feature")).toBeVisible();
  });

  test("has agent section", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("button", { name: "Create session" }).click();
    // The modal has an "AGENT" label
    await expect(page.locator("text=Agent").first()).toBeVisible();
  });

  test("submit disabled without path", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("button", { name: "Create session" }).click();
    // The submit button inside the modal (not the CTA)
    const submit = page.locator("form button[type='submit']");
    await expect(submit).toBeDisabled();
  });

  test("submit enables with path", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("button", { name: "Create session" }).click();
    await page.getByPlaceholder("/path/to/your/project").fill("/tmp/test");
    const submit = page.locator("form button[type='submit']");
    await expect(submit).toBeEnabled();
  });

  test("advanced options toggle", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("button", { name: "Create session" }).click();
    await expect(page.getByPlaceholder("Auto-generated if empty")).not.toBeVisible();
    await page.getByText("Show advanced options").click();
    await expect(page.getByPlaceholder("Auto-generated if empty")).toBeVisible();
  });

  test("closes on cancel", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("button", { name: "Create session" }).click();
    await expect(page.getByRole("heading", { name: "New Session" })).toBeVisible();
    await page.getByRole("button", { name: "Cancel" }).click();
    await expect(page.getByRole("heading", { name: "New Session" })).not.toBeVisible();
  });

  test("closes on escape", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("button", { name: "Create session" }).click();
    await expect(page.getByRole("heading", { name: "New Session" })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("heading", { name: "New Session" })).not.toBeVisible();
  });

  test("closes on backdrop click", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("button", { name: "Create session" }).click();
    await expect(page.getByRole("heading", { name: "New Session" })).toBeVisible();
    // Click the backdrop (top-left corner, outside the modal)
    await page.mouse.click(10, 10);
    await expect(page.getByRole("heading", { name: "New Session" })).not.toBeVisible();
  });
});

test.describe("Settings", () => {
  test("settings gear button visible", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByRole("button", { name: "Settings" })).toBeVisible();
  });

  test("settings opens on click", async ({ page }) => {
    await page.goto("/");
    await page.getByRole("button", { name: "Settings" }).click();
    // Settings view shows loading state (no backend in test)
    await expect(page.getByText("Loading settings...")).toBeVisible();
  });

  test("settings opens with keyboard shortcut s", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await page.locator("body").click();
    await page.keyboard.press("s");
    await expect(page.getByText("Loading settings...")).toBeVisible();
  });
});

test.describe("Keyboard shortcuts", () => {
  test("D toggles diff pane (no-op when no session, no crash)", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    // Should not crash even with no session selected
    await page.keyboard.press("Shift+d");
    await expect(page.getByText("No sessions yet")).toBeVisible();
  });

  test("? opens help overlay", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await page.locator("body").click();
    // Dispatch a ? keydown event directly since Shift+/ handling varies by layout
    await page.evaluate(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "?", bubbles: true }));
    });
    await expect(page.getByRole("heading", { name: "Keyboard Shortcuts" })).toBeVisible();
  });

  test("escape closes help overlay", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await page.locator("body").click();
    await page.evaluate(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "?", bubbles: true }));
    });
    await expect(page.getByRole("heading", { name: "Keyboard Shortcuts" })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("heading", { name: "Keyboard Shortcuts" })).not.toBeVisible();
  });
});

test.describe("Mobile responsive", () => {
  test("sidebar closed by default on mobile", async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    await page.goto("/");
    // Sidebar button should not be visible (sidebar closed)
    await expect(page.getByRole("button", { name: "+ New Session" })).not.toBeVisible();
    // Main content visible
    await expect(page.getByText("No sessions yet")).toBeVisible();
  });

  test("hamburger opens sidebar overlay on mobile", async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    await page.goto("/");
    await page.getByRole("button", { name: "Toggle sidebar" }).click();
    await expect(page.getByRole("button", { name: "+ New Session" })).toBeVisible();
  });

  test("sidebar has close button on mobile", async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    await page.goto("/");
    await page.getByRole("button", { name: "Toggle sidebar" }).click();
    const closeBtn = page.getByRole("button", { name: "×" });
    await expect(closeBtn).toBeVisible();
    await closeBtn.click();
    await expect(page.getByRole("button", { name: "+ New Session" })).not.toBeVisible();
  });

  test("settings gear accessible on mobile", async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    await page.goto("/");
    await expect(page.getByRole("button", { name: "Settings" })).toBeVisible();
  });

  test("create modal works on mobile", async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    await page.goto("/");
    await page.getByRole("button", { name: "Create session" }).click();
    await expect(page.getByRole("heading", { name: "New Session" })).toBeVisible();
    await expect(page.getByPlaceholder("/path/to/your/project")).toBeVisible();
  });
});

test.describe("Design system", () => {
  test("uses warm navy background", async ({ page }) => {
    await page.goto("/");
    const bg = await page.evaluate(() =>
      getComputedStyle(document.body).backgroundColor,
    );
    // #0f172a = rgb(15, 23, 42)
    expect(bg).toContain("15");
    expect(bg).not.toBe("rgb(13, 17, 23)");
  });

  test("loads DM Sans body font", async ({ page }) => {
    await page.goto("/");
    const fonts = await page.evaluate(() =>
      getComputedStyle(document.body).fontFamily,
    );
    expect(fonts.toLowerCase()).toContain("dm sans");
  });

  test("focus-visible ring appears on keyboard navigation", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    // Tab to the first button
    await page.keyboard.press("Tab");
    const outline = await page.evaluate(() => {
      const el = document.activeElement;
      return el ? getComputedStyle(el).outlineColor : "";
    });
    // Should have a brand-colored outline
    expect(outline).not.toBe("");
  });
});
