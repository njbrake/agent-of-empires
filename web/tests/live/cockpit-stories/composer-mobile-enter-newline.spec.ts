// User story: on mobile, plain Enter inserts a newline into the
// composer draft and does NOT dispatch the prompt.
//
// Playwright's iPhone 13 emulation runs on a host with a real
// pointing device, so `(any-pointer: fine)` resolves true and
// `detectMobileInput()` (Composer.tsx) would return false. Force the
// detection via addInitScript so the composer mounts in mobile mode
// and the user-visible newline behavior is actually exercised
// end-to-end, not just in a Vitest unit.

import { test as base, devices, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { waitForCockpitView, enableCockpitAndWait } from "../../helpers/cockpit";

base.use({ ...devices["iPhone 13"] });

base("mobile plain Enter inserts newline, does not dispatch", async ({
  page,
}, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-mobile-enter-newline" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const seeded = sessions.find((s) => s.title === "story-mobile-enter-newline");
    if (!seeded) throw new Error("seeded session missing");
    const sessionId = seeded.id;

    await enableCockpitAndWait(serve.baseUrl, sessionId);

    // Force detectMobileInput() to true. The real iPhone 13 emulation
    // gives (pointer: coarse) = true AND (any-pointer: fine) = true,
    // because Playwright drives the browser through a real desktop
    // pointer. Composer's detectMobileInput requires coarse && !anyFine,
    // so without this override we never hit the mobile branch.
    await page.addInitScript(() => {
      const real = window.matchMedia.bind(window);
      window.matchMedia = (q) => {
        const mql = real(q);
        if (q.includes("any-pointer: fine")) {
          Object.defineProperty(mql, "matches", { get: () => false });
        } else if (q.includes("pointer: coarse")) {
          Object.defineProperty(mql, "matches", { get: () => true });
        }
        return mql;
      };
    });

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);
    await waitForCockpitView(page);

    const composer = page.getByRole("textbox", { name: /Send a message/i });
    await composer.fill("line1");
    await composer.press("Enter");

    // Mobile branch inserts a literal newline at the caret and
    // suppresses Send. The composer should now hold "line1\n" (or
    // "line1\n" plus an empty line), NOT be cleared by a Send.
    await expect(composer).toHaveValue(/^line1\n/, { timeout: 5_000 });

    // And the fake-ACP agent should NOT have received a prompt. If it
    // had, "Hello from fake ACP agent." would render. Use a short
    // negative window: long enough that a real Send would have
    // surfaced, short enough not to dominate test wall time.
    await expect(page.getByText("Hello from fake ACP agent.")).toBeHidden({
      timeout: 2_000,
    });
  } finally {
    await serve.stop();
  }
});
