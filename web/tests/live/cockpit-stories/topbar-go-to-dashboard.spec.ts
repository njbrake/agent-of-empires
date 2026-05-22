// User story: click the topbar's "Go to dashboard" button to navigate
// home from a session view.
//
// Seeds a session, navigates to its session route, clicks the button,
// asserts the URL returns to "/".

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";

base("topbar Go to dashboard returns to /", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-go-dashboard" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const seeded = sessions.find((s) => s.title === "story-go-dashboard");
    if (!seeded) throw new Error("seeded session 'story-go-dashboard' missing");
    const sessionId = seeded.id;

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);
    await expect(page).toHaveURL(new RegExp(`/session/${sessionId}`), {
      timeout: 10_000,
    });

    await page.getByRole("button", { name: "Go to dashboard" }).click();
    await expect(page).toHaveURL(new RegExp(`${serve.baseUrl}/?$`), {
      timeout: 5_000,
    });
  } finally {
    await serve.stop();
  }
});
