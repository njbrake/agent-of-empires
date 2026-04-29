import { test, expect, type Page } from "@playwright/test";
import { mockTerminalApis } from "./helpers/terminal-mocks";

// Desktop viewport so the right diff/shell panel mounts inline (collapse
// default is window.innerWidth < 768).
test.use({ viewport: { width: 1280, height: 800 }, hasTouch: false });

async function openSession(page: Page) {
  await page.locator('button:has-text("pinch-test")').nth(1).click();
  // ContentSplit renders the right pane twice (desktop inline + mobile
  // overlay), so two PairedTerminal instances mount. Plus the agent.
  await expect(page.locator('[data-term="agent"]')).toHaveCount(1);
  await expect(page.locator('[data-term="paired"]')).toHaveCount(2);
  // Both terminals must finish their ensureSession / ensureTerminal
  // round-trips before the wterm textarea exists for focus.
  await expect(page.locator('[data-term="agent"] .wterm')).toBeVisible();
  await expect(page.locator('[data-term="paired"] .wterm').first()).toBeVisible();
}

// Returns "agent", "paired", or null based on which panel currently
// contains document.activeElement.
async function focusedPanel(page: Page): Promise<"agent" | "paired" | null> {
  return page.evaluate(() => {
    const active = document.activeElement;
    if (!active) return null;
    if (document.querySelector('[data-term="agent"]')?.contains(active)) {
      return "agent";
    }
    const paired = document.querySelectorAll('[data-term="paired"]');
    for (const p of paired) {
      if (p.contains(active)) return "paired";
    }
    return null;
  });
}

// Focus the wterm textarea inside the panel matching the data-term value.
// On desktop the visible paired panel is the one inside the `hidden md:flex`
// wrapper; .first() picks the desktop instance (its wrapper is rendered
// before the mobile one in JSX order).
async function focusPanel(page: Page, kind: "agent" | "paired") {
  await page
    .locator(`[data-term="${kind}"]`)
    .first()
    .locator("textarea")
    .focus();
}

test.describe("Cmd/Ctrl+` toggles terminal focus", () => {
  test("toggles between agent and paired with the right panel open", async ({
    page,
  }) => {
    await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page);

    // Anchor focus in the agent terminal so the toggle direction is
    // deterministic.
    await focusPanel(page, "agent");
    await expect.poll(() => focusedPanel(page)).toBe("agent");

    // Cmd+` (Control here; the handler accepts metaKey OR ctrlKey off-Mac,
    // and Linux Chromium reports navigator.platform = Linux).
    await page.keyboard.press("Control+`");
    await expect.poll(() => focusedPanel(page)).toBe("paired");

    // And back the other way.
    await page.keyboard.press("Control+`");
    await expect.poll(() => focusedPanel(page)).toBe("agent");
  });

  test("focuses paired terminal when nothing is focused yet", async ({
    page,
  }) => {
    await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page);

    // No explicit focus — document.activeElement is the body. The handler
    // treats "not in agent" as the toggle source, so the first press lands
    // focus in the paired panel.
    await page.keyboard.press("Control+`");
    await expect.poll(() => focusedPanel(page)).toBe("paired");
  });

  test("expands collapsed right panel and focuses paired terminal", async ({
    page,
  }) => {
    await mockTerminalApis(page);
    await page.goto("/");
    await openSession(page);

    // Collapse the right panel via the existing Ctrl+Alt+B shortcut, which
    // unmounts all PairedTerminal instances.
    await page.keyboard.press("Control+Alt+b");
    await expect(page.locator('[data-term="paired"]')).toHaveCount(0);

    // Focus the agent terminal so the toggle target becomes "paired".
    await focusPanel(page, "agent");
    await expect.poll(() => focusedPanel(page)).toBe("agent");

    // Press Cmd+` — sets the pending-focus latch, expands the right
    // panel, and once PairedTerminal mounts + ensureTerminal resolves,
    // the latch is consumed and focus moves to paired.
    await page.keyboard.press("Control+`");
    await expect(page.locator('[data-term="paired"]')).toHaveCount(2);
    await expect.poll(() => focusedPanel(page)).toBe("paired");
  });
});
