import type { Page } from "@playwright/test";

export async function clickSidebarSession(page: Page, title: string) {
  // Sidebar session rows are anchor tags (see WorkspaceSidebar.tsx). The
  // pre-link button-based fallback that used to live here was dead code by
  // the time it ran: if the link never appeared, the button locator never
  // did either, but its click had no timeout and would consume the rest
  // of the 30s test budget before erroring with a confusing "page closed"
  // message. Surfacing the link timeout directly gives a clean failure.
  //
  // 20s rather than 10s: at 6 mocked workers on a 4-vCPU runner with v8
  // coverage instrumentation, sidebar slide-in + sessions fetch + render
  // sometimes lose to scheduler contention past 10s. Two flakes in CI
  // (pinch-zoom MIN_FONT_SIZE, scrollback scroll-down clamp) traced to
  // exactly this race.
  const sessionLink = page.getByRole("link").filter({ hasText: title }).first();
  await sessionLink.waitFor({ state: "visible", timeout: 20_000 });
  await sessionLink.click();
}
