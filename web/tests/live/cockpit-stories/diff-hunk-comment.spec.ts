// User story: leave a comment on a diff hunk in a cockpit session.
//
// Cockpit-enabled session with an uncommitted file: click the file
// row → DiffFileViewer mounts → hover a line to surface the "+"
// gutter button → click it → CommentForm appears → write body, save.
// The persistent CommentsBanner then shows the comment count.

import { mkdirSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  resolveAoeBinary,
} from "../../helpers/aoeServe";
import { waitForCockpitView , enableCockpitAndWait } from "../../helpers/cockpit";

base("comment on a diff hunk persists in the comments banner", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    cockpit: true,
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
    seedFn: ({ home, env }) => {
      const projectDir = join(home, "project");
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
      writeFileSync(
        join(projectDir, "story.txt"),
        "line one\nline two\nline three\n",
      );
      const res = spawnSync(
        resolveAoeBinary(),
        ["add", projectDir, "-t", "story-hunk-comment", "-c", "claude"],
        { env },
      );
      if (res.status !== 0) {
        throw new Error(
          `aoe add failed: status=${res.status} stderr=${res.stderr?.toString() ?? "<none>"}`,
        );
      }
    },
  });

  try {
    const sessions = await listSessions(serve.baseUrl);
    const sessionId = sessions[0]!.id;

    await enableCockpitAndWait(serve.baseUrl, sessionId);

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);
    await waitForCockpitView(page);

    // Click the diff file row to open the file viewer.
    const fileRow = page.getByText("story.txt").first();
    await expect(fileRow).toBeVisible({ timeout: 15_000 });
    await fileRow.click();

    // Hover a diff line so the "+" gutter button reveals; selector
    // matches the aria-label pattern.
    const addBtn = page.getByRole("button", {
      name: /Add comment on .* line 1/,
    });
    await expect(addBtn.first()).toBeVisible({ timeout: 15_000 });
    await addBtn.first().click({ force: true });

    const composer = page.getByPlaceholder(/Leave a comment/i);
    await expect(composer).toBeVisible({ timeout: 5_000 });
    await composer.fill("looks good but rename this");
    await page.getByRole("button", { name: "Save", exact: true }).click();

    // The CommentsBanner / commentsCount surface should reflect one
    // saved comment.
    await expect(page.getByText(/1 comment/i)).toBeVisible({
      timeout: 10_000,
    });
  } finally {
    await serve.stop();
  }
});
