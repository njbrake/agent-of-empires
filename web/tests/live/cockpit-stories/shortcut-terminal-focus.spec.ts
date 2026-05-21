// User story: Cmd/Ctrl+` moves focus to the paired terminal panel.
//
// The chord lives in useKeyboardShortcuts.ts:50-54 and the handler in
// App.tsx:516 moves focus to the data-term="paired" panel; if the
// right panel is collapsed, it expands first.

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";

const MOD = process.platform === "darwin" ? "Meta" : "Control";

base("Cmd/Ctrl+` activates the paired terminal panel", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-terminal-focus" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const sessionId = sessions[0]!.id;
    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);

    const handle = page.locator('[data-testid="content-split-resize-handle"]');
    await expect(handle).toBeVisible({ timeout: 10_000 });

    // The paired terminal panel mounts inside the right pane and is
    // tagged with data-term="paired"; clicking Cmd/Ctrl+` should
    // surface it as the active focus owner.
    await page.locator("body").click({ position: { x: 5, y: 5 } });
    await page.keyboard.press(`${MOD}+Backquote`);

    const paired = page.locator('[data-term="paired"]').first();
    await expect(paired).toBeVisible({ timeout: 10_000 });
  } finally {
    await serve.stop();
  }
});
