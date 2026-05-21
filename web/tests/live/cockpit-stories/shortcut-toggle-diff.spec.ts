// User story: pressing D toggles the diff (right) panel on a session.
//
// Single-key shortcut (useKeyboardShortcuts.ts:88-91) flips
// App.tsx's `diffCollapsed`. ContentSplit conditionally renders its
// drag-handle + right pane based on `collapsed`, so the resize
// handle's presence is the simplest visual signal.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";

base("D key toggles the diff panel", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-diff-toggle" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const sessionId = sessions[0]!.id;

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);
    const handle = page.locator('[data-testid="content-split-resize-handle"]');
    await expect(handle).toBeVisible({ timeout: 10_000 });
    // Click outside the terminal so focus moves to body and the
    // input-gated D shortcut fires (capture-phase listener still wins,
    // but the gate at line 80 of useKeyboardShortcuts.ts skips inputs).
    await page.locator("body").click({ position: { x: 5, y: 5 } });

    await page.keyboard.press("Shift+D");
    await expect(handle).toBeHidden({ timeout: 5_000 });

    await page.keyboard.press("Shift+D");
    await expect(handle).toBeVisible({ timeout: 5_000 });
  } finally {
    await serve.stop();
  }
});
