// User story: on mobile, plain Enter does NOT dispatch the prompt.
//
// `decideEnterAction` in Composer.tsx returns "newline" for
// touch-primary devices on plain Enter; the textarea handles the
// keystroke natively and the composer never POSTs `/cockpit/prompt`.
// Asserting the absence of the default fake-ACP response is enough to
// prove the prompt path didn't fire; whether the keystroke produces a
// literal newline at the caret depends on browser quirks Playwright
// can't reproduce identically across pointer:coarse emulation.

import { test as base, expect, devices } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";
import { waitForCockpitView, enableCockpitAndWait } from "../../helpers/cockpit";

base.use({ ...devices["iPhone 13"] });

base("mobile plain Enter does not dispatch the prompt", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-mobile-enter" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const sessionId = sessions[0]!.id;

    await enableCockpitAndWait(serve.baseUrl, sessionId);

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);
    await waitForCockpitView(page);

    const composer = page.getByRole("textbox", { name: /Send a message/i });
    await composer.click();
    await composer.fill("first line");
    await composer.press("Enter");
    // Brief settle so any wrongly-dispatched WS round-trip would have
    // surfaced by now.
    await page.waitForTimeout(750);

    // Default fake turn would emit this text if the composer had sent
    // the prompt. Mobile Enter must NOT dispatch.
    await expect(page.getByText("Hello from fake ACP agent.")).toHaveCount(0);
    // The composer should still hold the typed text rather than be
    // cleared (which is what dispatch would do).
    const value = await composer.inputValue();
    expect(value.length).toBeGreaterThan(0);
    expect(value).toContain("first line");
  } finally {
    await serve.stop();
  }
});
