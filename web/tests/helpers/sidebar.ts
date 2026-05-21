import type { Page } from "@playwright/test";

export async function clickSidebarSession(page: Page, title: string) {
  const sessionLink = page.getByRole("link").filter({ hasText: title }).first();
  try {
    // 10s rather than 5s: at 6 mocked-suite workers on a 4-vCPU runner,
    // the React render + sidebar slide-in can lose to scheduler contention
    // and miss a 5s wait, which manifests as the surrounding test hitting
    // its 30s timeout.
    await sessionLink.waitFor({ state: "visible", timeout: 10_000 });
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
