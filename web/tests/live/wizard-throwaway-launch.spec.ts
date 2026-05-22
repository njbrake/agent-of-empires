// User story: launching a throwaway session from the wizard creates a
// real session on the server with `throwaway: true` and a `project_path`
// under the OS temp directory. Closes #1324.

import { tmpdir } from "node:os";
import { test as base, expect } from "@playwright/test";
import { listSessions, spawnAoeServe } from "../helpers/aoeServe";

base("throwaway happy path: launch creates a temp-dir session", async ({ page }, testInfo) => {
  const serve = await spawnAoeServe({
    authMode: "none",
    workerIndex: testInfo.workerIndex,
    parallelIndex: testInfo.parallelIndex,
  });

  try {
    await page.goto(serve.baseUrl);
    await page
      .getByRole("button", { name: "New session", exact: true })
      .first()
      .click();

    const wizard = page.locator(
      'div.fixed.inset-0.z-50:has(h1:has-text("New session"))',
    );
    await expect(wizard).toBeVisible({ timeout: 15_000 });

    // ProjectStep: enable throwaway, advance.
    await wizard
      .getByRole("switch", { name: "Skip project folder" })
      .click();
    await wizard.getByRole("button", { name: "Next" }).click();

    // SessionStep: title is auto-generated; just advance.
    await expect(
      wizard.getByRole("heading", { name: "Name your session", exact: true }),
    ).toBeVisible({ timeout: 10_000 });
    await wizard.getByRole("button", { name: "Next" }).click();

    // AgentStep: claude default; advance.
    await wizard.getByRole("button", { name: "Next" }).click();

    // ReviewStep: project label must say "Temporary directory ..."; Launch.
    await expect(
      wizard.getByText(/Temporary directory \(provisioned on create\)/),
    ).toBeVisible({ timeout: 10_000 });
    await wizard.getByRole("button", { name: /Launch session/ }).click();

    // Server-side: a session exists, marked throwaway, with a project_path
    // under tmpdir() and the aoe-throwaway- basename prefix.
    await expect
      .poll(async () => (await listSessions(serve.baseUrl)).length, {
        timeout: 15_000,
      })
      .toBeGreaterThan(0);

    const sessions = await listSessions(serve.baseUrl);
    expect(sessions).toHaveLength(1);
    const session = sessions[0]!;
    expect(session.throwaway).toBe(true);
    const projectPath = session.project_path as string;
    expect(projectPath.startsWith(tmpdir())).toBe(true);
    expect(projectPath).toContain("aoe-throwaway-");
  } finally {
    await serve.stop();
  }
});
