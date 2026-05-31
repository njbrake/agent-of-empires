// Live coverage for cross-process file-watch propagation in the dashboard:
//   - Spawn `aoe serve` against an isolated HOME.
//   - Seed one session via `aoe session add` (peer subprocess).
//   - Open the dashboard; assert the seeded session is visible.
//   - Issue a peer `aoe session rename` to mutate the on-disk
//     `sessions.json` from a different process.
//   - Assert the dashboard reflects the new title within the watcher
//     propagation budget (1.5s typical, 3s ceiling).
//
// Verifies the server-consumer migration (server-migration doc §8.2):
// `Storage::update` from a peer process triggers the kernel watcher
// in the daemon, fans into `disk_changed`, and the consumer task
// reloads `state.instances` so the dashboard sees the change without
// waiting for the 2s `status_poll_loop` tick.

import { test, expect } from "@playwright/test";
import { spawnSync } from "node:child_process";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
  resolveAoeBinary,
} from "../helpers/aoeServe";

const aoeBinary = resolveAoeBinary();

test.describe.serial("file-watch peer propagation", () => {
  test("peer rename surfaces within the watcher budget", async ({ page }, ti) => {
    const serve = await spawnAoeServe({
      authMode: "none",
      workerIndex: ti.workerIndex,
      parallelIndex: ti.parallelIndex,
      seedFn: seedSessionViaAoeAdd({ title: "peer-source" }),
    });
    try {
      const id = (await listSessions(serve.baseUrl))[0]!.id as string;
      await page.goto(`${serve.baseUrl}/`);
      await expect(page.getByText("peer-source")).toBeVisible({ timeout: 10_000 });

      spawnSync(aoeBinary, ["session", "rename", id, "peer-target"], {
        env: serve.env,
        stdio: "inherit",
      });

      // 3s ceiling absorbs FSEvents coalescing on macOS while still being
      // tighter than the 2s poll fallback (gives confidence the kernel
      // watcher actually fired).
      await expect(page.getByText("peer-target")).toBeVisible({ timeout: 3_000 });
    } finally {
      await serve.stop();
    }
  });
});
