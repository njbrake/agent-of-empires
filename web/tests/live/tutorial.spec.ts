// User stories (issue #1513): the first-run tutorial auto-launches on a fresh
// browser, is skippable, persists "seen" so it does not nag on reload, and is
// re-triggerable from the TopBar overflow menu.
//
// A fresh `aoe serve` $HOME has no sessions, so the app lands on the empty
// dashboard and the dashboard-scope tour auto-launches (Playwright is a
// fine-pointer client, so the coarse-pointer suppression does not apply).
import { test as base, expect } from "@playwright/test";
import { spawnAoeServe } from "../helpers/aoeServe";

// First dashboard step's title (TOUR_STEPS[0] = topbar -> "Command bar").
const FIRST_STEP = "Command bar";

base("first-run tutorial: auto-launch, skip, persist, re-trigger", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
  });

  try {
    // Auto-launch is suppressed in automated sessions (navigator.webdriver) so
    // the spotlight overlay never intercepts clicks in the rest of the suite.
    // This spec is the one place that exercises auto-launch, so present as a
    // real (non-automated) browser. Persists across reloads/navigations.
    await page.addInitScript(() => {
      Object.defineProperty(navigator, "webdriver", { get: () => false });
    });

    await page.goto(serve.baseUrl);

    // Story 1: auto-launches on first load, with a Skip button on the step.
    await expect(page.getByText(FIRST_STEP)).toBeVisible({ timeout: 10_000 });
    const skip = page.getByRole("button", { name: "Skip" });
    await expect(skip).toBeVisible();

    // Skipping closes the tour and records the seen flag.
    await skip.click();
    await expect(page.getByText(FIRST_STEP)).toBeHidden();
    await expect
      .poll(() => page.evaluate(() => localStorage.getItem("aoe-tour-seen")))
      .toBe("1");

    // Story 1 (persistence): a reload must not auto-launch again.
    await page.reload();
    await expect(page.getByRole("button", { name: "Go to dashboard" })).toBeVisible();
    await expect(page.getByText(FIRST_STEP)).toBeHidden();

    // Story 2: re-trigger from the fixed entry point (TopBar overflow menu).
    await page.getByRole("button", { name: "More options" }).click();
    await page.getByRole("menuitem", { name: "Show tutorial" }).click();
    await expect(page.getByText(FIRST_STEP)).toBeVisible({ timeout: 10_000 });

    // The flag stays set after a manual re-trigger, so the next reload is quiet.
    expect(await page.evaluate(() => localStorage.getItem("aoe-tour-seen"))).toBe("1");
  } finally {
    await serve.stop();
  }
});
