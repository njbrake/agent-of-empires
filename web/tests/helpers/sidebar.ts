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
  // On mobile the sidebar slides in from the left; while it is closed or
  // mid-transition, the row's bounding box has a negative x. Playwright's
  // `visible` check passes (non-zero box, not display:none) but `.click()`
  // then loops on "element is outside of the viewport" for the full 30s
  // test timeout. Wait until the box settles inside the viewport.
  await page.waitForFunction(
    (linkTitle) => {
      const links = Array.from(document.querySelectorAll("a"));
      const link = links.find((a) => a.textContent?.includes(linkTitle));
      if (!link) return false;
      const r = link.getBoundingClientRect();
      return r.x >= 0 && r.y >= 0 && r.width > 0 && r.height > 0;
    },
    title,
    { timeout: 10_000 },
  );
  await sessionLink.click();
}

// Mobile specs (`devices['iPhone 13']`) open the workspace sidebar before
// clicking a session row. The old recipe `if (await toggle.isVisible())`
// is a single non-retrying snapshot that races with React hydration on
// loaded CI workers, so the toggle click is skipped and the sidebar stays
// closed; the subsequent row click then times out on actionability.
//
// This helper is deterministic: it waits for the toggle to mount, probes
// the sidebar's current x via the session-row testid, and only clicks the
// toggle when the sidebar is fully closed (x < -row width / 2). It then
// blocks until the slide-in transition settles the box at x >= 0.
export async function openMobileSidebar(page: Page) {
  const toggle = page.getByRole("button", { name: "Toggle sidebar" });
  await toggle.waitFor({ state: "visible", timeout: 10_000 });
  const probe = page.getByTestId("sidebar-session-row").first();
  await probe.waitFor({ state: "attached", timeout: 10_000 });
  const initial = await probe.boundingBox();
  if (!initial || initial.x < 0) {
    await toggle.click();
  }
  await page.waitForFunction(
    () => {
      const row = document.querySelector(
        '[data-testid="sidebar-session-row"]',
      );
      if (!row) return false;
      const r = (row as HTMLElement).getBoundingClientRect();
      return r.x >= 0 && r.width > 0;
    },
    null,
    { timeout: 5_000 },
  );
}
