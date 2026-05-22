// User story: drag the right-panel resize handle; width persists
// across reload.
//
// ContentSplit.tsx exposes data-testid="content-split-resize-handle".
// The global mouseup handler writes the new width to localStorage key
// "aoe-split-ratio".

import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";

base("right panel width persists across reload after dragging the handle", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: seedSessionViaAoeAdd({ title: "story-right-resize" }),
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const seeded = sessions.find((s) => s.title === "story-right-resize");
    if (!seeded) throw new Error("seeded session 'story-right-resize' missing");
    const sessionId = seeded.id;
    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);

    const handle = page.locator('[data-testid="content-split-resize-handle"]');
    await expect(handle).toBeVisible({ timeout: 10_000 });

    const box = await handle.boundingBox();
    if (!box) throw new Error("handle has no bounding box");

    const startX = box.x + box.width / 2;
    const y = box.y + box.height / 2;
    const targetX = startX - 80;

    const storedBefore = await page.evaluate(() =>
      localStorage.getItem("aoe-split-ratio"),
    );

    await page.mouse.move(startX, y);
    await page.mouse.down();
    await page.mouse.move(targetX, y, { steps: 5 });
    await page.mouse.up();

    const storedAfter = await page.evaluate(() =>
      localStorage.getItem("aoe-split-ratio"),
    );
    expect(storedAfter).not.toBeNull();
    const widthAfter = parseInt(storedAfter!, 10);
    expect(widthAfter).toBeGreaterThanOrEqual(280);
    // Drag must change the persisted ratio.
    expect(storedAfter).not.toBe(storedBefore);

    await page.reload();
    const storedReloaded = await page.evaluate(() =>
      localStorage.getItem("aoe-split-ratio"),
    );
    expect(storedReloaded).toBe(storedAfter);
  } finally {
    await serve.stop();
  }
});
