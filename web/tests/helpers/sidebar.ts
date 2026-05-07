import type { Page } from "@playwright/test";

export async function clickSidebarSession(page: Page, title: string) {
  const sessionLink = page.getByRole("link").filter({ hasText: title }).first();
  try {
    await sessionLink.waitFor({ state: "visible", timeout: 5_000 });
    await sessionLink.click();
    return;
  } catch {
    // Fall back to the pre-link sidebar implementation below.
  }

  // Older sidebar implementations rendered the repo-group header and the
  // nested session row as buttons with the same text. The second match is the
  // actual session row.
  await page.locator("button").filter({ hasText: title }).nth(1).click();
}
