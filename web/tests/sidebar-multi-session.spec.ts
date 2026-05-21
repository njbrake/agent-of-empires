import { test, expect } from "./helpers/mockedTest";
import { Page } from "@playwright/test";

// Two sessions sharing the same `(project_path, branch=null)` collapsed
// behind `workspace.sessions[0]` and only one rendered in the sidebar.
// useWorkspaces now splits null-branch sessions into one workspace per
// session so both rows appear. See #956.

interface MockSession {
  id: string;
  title: string;
  project_path: string;
  branch: string | null;
  status?: string;
}

async function mockApis(page: Page, sessions: MockSession[]) {
  await page.route("**/api/login/status", (r) =>
    r.fulfill({ json: { required: false, authenticated: true } }),
  );
  await page.route("**/api/sessions", (r) => {
    if (r.request().method() !== "GET") return r.fulfill({ status: 400 });
    return r.fulfill({
      json: {
        sessions: sessions.map((s) => ({
          id: s.id,
          title: s.title,
          project_path: s.project_path,
          group_path: s.project_path,
          tool: "claude",
          status: s.status ?? "Idle",
          yolo_mode: false,
          created_at: new Date().toISOString(),
          last_accessed_at: null,
          last_error: null,
          branch: s.branch,
          main_repo_path: null,
          is_sandboxed: false,
          has_terminal: true,
          profile: "default",
          workspace_repos: [],
        })),
        workspace_ordering: [],
      },
    });
  });
  for (const path of [
    "settings",
    "themes",
    "agents",
    "profiles",
    "groups",
    "devices",
    "docker/status",
    "about",
  ]) {
    await page.route(`**/api/${path}`, (r) =>
      r.fulfill({ json: path === "docker/status" ? {} : [] }),
    );
  }
}

test.describe("Sidebar multi-session (#956)", () => {
  test("renders one row per null-branch session on the same project_path", async ({
    page,
  }) => {
    await mockApis(page, [
      {
        id: "sess-a",
        title: "Ethiopians",
        project_path: "/tmp/agent-of-empires",
        branch: null,
      },
      {
        id: "sess-b",
        title: "Celts",
        project_path: "/tmp/agent-of-empires",
        branch: null,
      },
    ]);
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await expect(page.locator("header")).toBeVisible();
    await expect(page.getByRole("link", { name: /Ethiopians/i })).toBeVisible();
    await expect(page.getByRole("link", { name: /Celts/i })).toBeVisible();
  });

  test("clicking a session row uses client-side navigation", async ({
    page,
  }) => {
    await mockApis(page, [
      {
        id: "sess-a",
        title: "Ethiopians",
        project_path: "/tmp/agent-of-empires",
        branch: null,
      },
      {
        id: "sess-b",
        title: "Celts",
        project_path: "/tmp/agent-of-empires",
        branch: null,
      },
    ]);
    let sessionDocumentRequests = 0;
    page.on("request", (request) => {
      if (
        request.resourceType() === "document" &&
        /\/session\/sess-[ab]$/.test(new URL(request.url()).pathname)
      ) {
        sessionDocumentRequests += 1;
      }
    });

    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await expect(page.locator("header")).toBeVisible();
    const row = page.getByRole("link", { name: /Ethiopians/i });

    await expect(row).toHaveJSProperty("tagName", "A");
    await expect(row).toHaveAttribute("href", /\/session\/sess-a$/);
    await row.click();

    await expect(page).toHaveURL(/\/session\/sess-a$/);
    expect(sessionDocumentRequests).toBe(0);
  });

  test("a deliberate desktop click does not get swallowed as a drag", async ({
    page,
  }) => {
    await mockApis(page, [
      {
        id: "sess-a",
        title: "Ethiopians",
        project_path: "/tmp/agent-of-empires",
        branch: null,
      },
      {
        id: "sess-b",
        title: "Celts",
        project_path: "/tmp/agent-of-empires",
        branch: null,
      },
    ]);

    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/session/sess-b");
    await expect(page.locator("header")).toBeVisible();
    await expect(page).toHaveURL(/\/session\/sess-b$/);

    const row = page.getByRole("link", { name: /Ethiopians/i });
    const box = await row.boundingBox();
    expect(box).not.toBeNull();

    await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
    await page.mouse.down();
    await page.waitForTimeout(220);
    await page.mouse.up();
    await page.waitForTimeout(16);

    expect(await row.getAttribute("class")).toContain("border-brand-600");
    await expect(page).toHaveURL(/\/session\/sess-a$/);
  });

  test("deleting rows are disabled for pointer and keyboard activation", async ({
    page,
  }) => {
    await mockApis(page, [
      {
        id: "sess-a",
        title: "Ethiopians",
        project_path: "/tmp/agent-of-empires",
        branch: null,
        status: "Deleting",
      },
      {
        id: "sess-b",
        title: "Celts",
        project_path: "/tmp/agent-of-empires",
        branch: null,
      },
    ]);

    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/session/sess-b");
    await expect(page.locator("header")).toBeVisible();
    const row = page.getByRole("link", { name: /Ethiopians/i });

    await expect(row).toHaveAttribute("aria-disabled", "true");
    await expect(row).toHaveAttribute("tabindex", "-1");

    await row.evaluate((el) => (el as HTMLElement).click());
    await expect(page).toHaveURL(/\/session\/sess-b$/);

    await row.evaluate((el) => {
      el.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    });
    await expect(page).toHaveURL(/\/session\/sess-b$/);
  });

  test("collapsing still applies when sessions share a non-null branch (worktree)", async ({
    page,
  }) => {
    // Two sessions on the same explicit worktree branch DO still collapse;
    // the fix only targets the null-branch (no-worktree) case. This matches
    // the issue's option #2.
    await mockApis(page, [
      {
        id: "sess-a",
        title: "Ethiopians",
        project_path: "/tmp/agent-of-empires",
        branch: "feature/x",
      },
      {
        id: "sess-b",
        title: "Celts",
        project_path: "/tmp/agent-of-empires",
        branch: "feature/x",
      },
    ]);
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await expect(page.locator("header")).toBeVisible();
    const branchRow = page.getByRole("link", { name: /feature\/x/i });
    await expect(branchRow).toHaveCount(1);
  });

  test("distinct branches render their own rows (regression guard)", async ({
    page,
  }) => {
    await mockApis(page, [
      {
        id: "sess-a",
        title: "Italians",
        project_path: "/tmp/agent-of-empires",
        branch: "feature/a",
      },
      {
        id: "sess-b",
        title: "Magyars",
        project_path: "/tmp/agent-of-empires",
        branch: "feature/b",
      },
    ]);
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");
    await expect(page.locator("header")).toBeVisible();
    await expect(page.getByRole("link", { name: /feature\/a/i })).toBeVisible();
    await expect(page.getByRole("link", { name: /feature\/b/i })).toBeVisible();
  });
});
