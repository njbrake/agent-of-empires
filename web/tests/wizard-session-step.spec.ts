import { test, expect } from "./helpers/mockedTest";
import { Page } from "@playwright/test";

// Wizard Session step (#1219). Covers title, worktree toggle gating
// of the branch input + Advanced section, and the group field. The
// "Attach to existing branch" toggle has its own dedicated spec
// (wizard-attach-existing.spec.ts, #969); the base-branch picker has
// wizard-base-branch.spec.ts (#948). Don't duplicate those here.

async function mockApis(page: Page) {
  await page.route("**/api/login/status", (r) =>
    r.fulfill({ json: { required: false, authenticated: true } }),
  );
  for (const path of [
    "settings",
    "themes",
    "profiles",
    "groups",
    "devices",
    "about",
    "system/update-status",
  ]) {
    await page.route(`**/api/${path}`, (r) =>
      r.fulfill({
        json:
          path === "settings" || path === "about" || path === "system/update-status"
            ? {}
            : [],
      }),
    );
  }
  await page.route("**/api/docker/status", (r) =>
    r.fulfill({ json: { available: false, runtime: null } }),
  );
  await page.route("**/api/agents", (r) =>
    r.fulfill({
      json: [
        { name: "claude", binary: "claude", host_only: false, installed: true, install_hint: "" },
      ],
    }),
  );
  await page.route("**/api/sessions", (r) => {
    if (r.request().method() === "GET") {
      return r.fulfill({
        json: {
          sessions: [
            {
              id: "seed-session",
              title: "seed",
              project_path: "/tmp/example",
              group_path: "/tmp",
              tool: "claude",
              status: "Idle",
              yolo_mode: false,
              created_at: new Date().toISOString(),
              last_accessed_at: null,
              last_error: null,
              branch: null,
              main_repo_path: null,
              is_sandboxed: false,
              has_terminal: true,
              profile: "default",
              workspace_repos: [],
            },
          ],
          workspace_ordering: [],
        },
      });
    }
    return r.fulfill({ json: { session: { id: "new-session" } } });
  });
}

async function openSessionStep(page: Page) {
  await page.locator("body").click();
  await page.keyboard.press("n");
  await expect(page.getByRole("heading", { name: "New session" })).toBeVisible();
  const recent = page.getByRole("button").filter({ hasText: "/tmp/example" }).first();
  await recent.waitFor({ state: "visible", timeout: 5000 });
  await recent.click();
  const next = page.getByRole("button", { name: "Next" });
  await expect(next).toBeEnabled();
  await next.click();
  await expect(page.getByText("Name your session")).toBeVisible();
}

test.describe("Wizard session step (#1219)", () => {
  test("title input is empty by default and updates as the user types", async ({ page }) => {
    await mockApis(page);
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/");
    await openSessionStep(page);
    const titleInput = page.getByPlaceholder("Auto-generated if empty");
    await expect(titleInput).toHaveValue("");
    await titleInput.fill("my-feature");
    await expect(titleInput).toHaveValue("my-feature");
  });

  test("worktree toggle is on by default and gates the branch input + Advanced section", async ({
    page,
  }) => {
    await mockApis(page);
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/");
    await openSessionStep(page);
    const worktreeToggle = page.getByRole("switch", { name: /Create a worktree/ });
    await expect(worktreeToggle).toHaveAttribute("aria-checked", "true");
    // Branch input visible while worktree is on.
    await expect(page.getByPlaceholder("Uses session title if empty")).toBeVisible();
    await expect(page.getByRole("button", { name: /Advanced/ })).toBeVisible();
    // Flip off: branch input + Advanced disappear.
    await worktreeToggle.click();
    await expect(worktreeToggle).toHaveAttribute("aria-checked", "false");
    await expect(page.getByPlaceholder("Uses session title if empty")).toHaveCount(0);
    await expect(page.getByRole("button", { name: /Advanced/ })).toHaveCount(0);
  });

  test("branch input mirrors the slugified title while not manually edited", async ({
    page,
  }) => {
    await mockApis(page);
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/");
    await openSessionStep(page);
    await page.getByPlaceholder("Auto-generated if empty").fill("My Cool Feature");
    const branchInput = page.getByPlaceholder("Uses session title if empty");
    // SET_FIELD on title cascades into worktreeBranch via slugifyBranch().
    await expect(branchInput).toHaveValue("my-cool-feature");
  });

  test("submit sends worktree_branch derived from title when the user leaves branch blank", async ({
    page,
  }) => {
    await mockApis(page);
    let captured: { worktree_branch?: string } | null = null;
    await page.route("**/api/sessions", (r) => {
      if (r.request().method() === "POST") {
        captured = JSON.parse(r.request().postData() || "{}");
        return r.fulfill({ json: { session: { id: "new-session" } } });
      }
      return r.fulfill({
        json: {
          sessions: [
            {
              id: "seed-session",
              title: "seed",
              project_path: "/tmp/example",
              group_path: "/tmp",
              tool: "claude",
              status: "Idle",
              yolo_mode: false,
              created_at: new Date().toISOString(),
              last_accessed_at: null,
              last_error: null,
              branch: null,
              main_repo_path: null,
              is_sandboxed: false,
              has_terminal: true,
              profile: "default",
              workspace_repos: [],
            },
          ],
          workspace_ordering: [],
        },
      });
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/");
    await openSessionStep(page);
    await page.getByPlaceholder("Auto-generated if empty").fill("Cool Feature");
    await page.getByRole("button", { name: /Next/ }).click();
    await page.getByRole("button", { name: /Next/ }).click();
    await page.getByRole("button", { name: /Launch session/ }).click();
    await expect.poll(() => captured?.worktree_branch).toBe("cool-feature");
  });

  test("group input propagates to the create-session POST body", async ({ page }) => {
    await mockApis(page);
    let captured: { group?: string } | null = null;
    await page.route("**/api/sessions", (r) => {
      if (r.request().method() === "POST") {
        captured = JSON.parse(r.request().postData() || "{}");
        return r.fulfill({ json: { session: { id: "new-session" } } });
      }
      return r.fulfill({
        json: {
          sessions: [
            {
              id: "seed-session",
              title: "seed",
              project_path: "/tmp/example",
              group_path: "/tmp",
              tool: "claude",
              status: "Idle",
              yolo_mode: false,
              created_at: new Date().toISOString(),
              last_accessed_at: null,
              last_error: null,
              branch: null,
              main_repo_path: null,
              is_sandboxed: false,
              has_terminal: true,
              profile: "default",
              workspace_repos: [],
            },
          ],
          workspace_ordering: [],
        },
      });
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto("/");
    await openSessionStep(page);
    await page
      .getByPlaceholder("Optional, for organizing related sessions")
      .fill("backend");
    await page.getByRole("button", { name: /Next/ }).click();
    await page.getByRole("button", { name: /Next/ }).click();
    await page.getByRole("button", { name: /Launch session/ }).click();
    await expect.poll(() => captured?.group).toBe("backend");
  });
});
