// Live coverage for `PUT /api/workspace-ordering`. The mocked spec at
// `web/tests/sidebar-drag-reorder.spec.ts` already drives the press-and-
// hold drag against a stubbed `/api/workspace-ordering`; this spec hits
// the real backend so a wire-format drift on either side blows up
// before reaching `main`.
//
// Why not "session-group-move": #1220 originally asked for a
// `PATCH /api/groups/:id/sessions` spec, but no such endpoint exists.
// Sessions belong to the group derived from their `project_path` (see
// `useRepoGroups` in `web/src/hooks/useRepoGroups.ts`), and drag is
// constrained to a single group per #1169. The "move a session to a
// different group" flow has no UI or API to exercise; the actually
// movable thing is the workspace ordering, covered here.
//
// The press-and-hold mouse sequence mirrors `sidebar-drag-reorder.spec.ts`
// (mouse.move → mouse.down → 250ms wait → mouse.move with steps →
// mouse.up); dnd-kit's MouseSensor uses `{ delay: 150, tolerance: 8 }`,
// so the hold has to outlast the activation window without moving more
// than 8px.

import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  resolveAoeBinary,
} from "../helpers/aoeServe";

function seedThreeSessionsInOneRepo(titles: [string, string, string]) {
  return ({ home, env }: { home: string; shimBin: string; env: NodeJS.ProcessEnv }) => {
    const binary = resolveAoeBinary();
    const projectDir = join(home, "repo");
    mkdirSync(projectDir, { recursive: true });
    spawnSync("git", ["init", "-q"], { cwd: projectDir });
    spawnSync("git", ["commit", "--allow-empty", "-q", "-m", "init"], {
      cwd: projectDir,
      env: {
        ...env,
        GIT_AUTHOR_NAME: "t",
        GIT_AUTHOR_EMAIL: "t@t",
        GIT_COMMITTER_NAME: "t",
        GIT_COMMITTER_EMAIL: "t@t",
      },
    });
    for (const title of titles) {
      const res = spawnSync(
        binary,
        ["add", projectDir, "-t", title, "-c", "claude"],
        { env },
      );
      if (res.status !== 0) {
        throw new Error(
          `aoe add failed for ${title}: status=${res.status} stderr=${res.stderr?.toString() ?? "<none>"}`,
        );
      }
    }
  };
}

async function readVisibleSessionTitles(page: import("@playwright/test").Page): Promise<string[]> {
  return page.evaluate(() => {
    const rows = Array.from(
      document.querySelectorAll<HTMLElement>(
        "[data-testid='sidebar-session-row']",
      ),
    );
    // Scope to the label span specifically (see WorkspaceSidebar.tsx:587).
    // A bare `[title]` selector can pick up a Wakeup or Plan chip if either
    // ever renders on the row.
    return rows
      .map(
        (r) =>
          r.querySelector("span.truncate[title]")?.getAttribute("title") ?? "",
      )
      .filter(Boolean);
  });
}

base.describe("workspace ordering live round-trip (#1220)", () => {
  base("press-and-hold drag reorders and round-trips PUT /api/workspace-ordering", async ({ page }, testInfo) => {
    // `aoe add` records workspace ids as `<project_path>::<title>` (see
    // `merge_workspace_ordering` in `src/server/api/sessions.rs`); the
    // server prepends new ids newest-first, so the seeded order in
    // arrival sequence is gamma at the top, beta in the middle, alpha
    // at the bottom.
    const serve = await spawnAoeServe({
      authMode: "none",
      workerIndex: testInfo.workerIndex,
      parallelIndex: testInfo.parallelIndex,
      seedFn: seedThreeSessionsInOneRepo(["alpha", "beta", "gamma"]),
    });

    try {
      const seeded = await listSessions(serve.baseUrl);
      expect(seeded).toHaveLength(3);

      const puts: string[][] = [];
      await page.route("**/api/workspace-ordering", (route) => {
        if (route.request().method() === "PUT") {
          const body = route.request().postDataJSON() as { order?: string[] };
          if (Array.isArray(body.order)) puts.push(body.order);
        }
        return route.continue();
      });

      await page.setViewportSize({ width: 1280, height: 720 });
      await page.goto(`${serve.baseUrl}/`);

      // The sidebar paints after a `GET /api/sessions` round-trip, so
      // poll rather than reading once. Three rows is the steady state.
      await expect
        .poll(() => readVisibleSessionTitles(page), { timeout: 8_000 })
        .toEqual(expect.arrayContaining(["alpha", "beta", "gamma"]));
      const initial = await readVisibleSessionTitles(page);
      expect(initial).toHaveLength(3);
      expect(new Set(initial)).toEqual(new Set(["alpha", "beta", "gamma"]));

      // Press the bottom wrapper, hold past the 150ms activation delay,
      // then drag onto the top wrapper.
      const wrappers = page.locator(
        "[aria-roledescription='Press and hold to reorder']",
      );
      await expect(wrappers).toHaveCount(3);

      const sourceBox = await wrappers.nth(2).boundingBox();
      const targetBox = await wrappers.nth(0).boundingBox();
      if (!sourceBox || !targetBox) throw new Error("row boxes missing");

      await page.mouse.move(
        sourceBox.x + sourceBox.width - 4,
        sourceBox.y + sourceBox.height / 2,
      );
      await page.mouse.down();
      await page.waitForTimeout(250);
      await page.mouse.move(
        targetBox.x + targetBox.width / 2,
        targetBox.y + targetBox.height / 2,
        { steps: 12 },
      );

      // Source row gets the active-drag amber ring; assert before
      // releasing so a future visual regression trips the test.
      const sourceClass = await wrappers.nth(2).getAttribute("class");
      expect(sourceClass ?? "").toContain("ring-2");

      await page.mouse.up();

      // After release, the bottom row is now at the top and the PUT
      // body reflects the new full flat order.
      await expect
        .poll(() => readVisibleSessionTitles(page), { timeout: 4_000 })
        .toEqual([initial[2], initial[0], initial[1]]);

      // Reconstruct expected workspace ids from the seeded sessions in
      // the dragged order. Workspace ids without a branch are
      // `<project_path>::__session__::<session_id>` (see
      // `useWorkspaces.ts:31`).
      const byTitle = new Map<string, string>(
        seeded.map((s) => [
          s.title as string,
          `${(s.project_path as string).replace(/\/+$/, "")}::__session__::${s.id as string}`,
        ]),
      );

      await expect
        .poll(() => puts.at(-1), { timeout: 4_000 })
        .toEqual([
          byTitle.get(initial[2]!),
          byTitle.get(initial[0]!),
          byTitle.get(initial[1]!),
        ]);

      // After the drag completes, the server's persisted ordering
      // mirrors the PUT body. Probe via `GET /api/sessions` which
      // returns the merged ordering envelope.
      const after = await fetch(`${serve.baseUrl}/api/sessions`);
      const body = (await after.json()) as { workspace_ordering: string[] };
      expect(body.workspace_ordering.slice(0, 3)).toEqual(puts.at(-1));
    } finally {
      await serve.stop();
    }
  });
});
