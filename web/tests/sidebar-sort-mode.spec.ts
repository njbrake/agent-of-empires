// Mocked-Playwright coverage for the sidebar sort-mode toggle (#1418).
//
// Drives the WorkspaceSidebar header toggle through its two states
// (manual, lastActivity) against fully-stubbed /api responses, so the
// only thing under test is the React + dnd-kit wiring:
//   1. Default is manual; rows honor server-supplied workspace_ordering.
//   2. Toggling to lastActivity reorders client-side by the issue's
//      sort key (max of last_accessed_at, idle_entered_at, created_at).
//   3. localStorage persists across reloads; the toggle starts pressed.
//   4. Press-and-hold in lastActivity mode does not lift or PUT.
//   5. Multi-repo group stays pinned at the bottom in lastActivity mode.
//
// Live persistence semantics (real aoe serve, real last_accessed_at
// bump) live in the matching tests/live/sidebar-sort-mode.spec.ts.

import { test, expect } from "./helpers/mockedTest";
import { Page } from "@playwright/test";

interface MockSession {
  id: string;
  title: string;
  project_path: string;
  branch: string | null;
  created_at: string;
  last_accessed_at?: string | null;
  idle_entered_at?: string | null;
  workspace_repos?: { name: string; source_path: string; branch: string }[];
}

function sessionResponse(s: MockSession) {
  return {
    id: s.id,
    title: s.title,
    project_path: s.project_path,
    group_path: s.project_path,
    tool: "claude",
    status: "Idle",
    yolo_mode: false,
    created_at: s.created_at,
    last_accessed_at: s.last_accessed_at ?? null,
    idle_entered_at: s.idle_entered_at ?? null,
    last_error: null,
    branch: s.branch,
    main_repo_path: null,
    is_sandboxed: false,
    has_terminal: true,
    profile: "default",
    workspace_repos: s.workspace_repos ?? [],
  };
}

async function mockApis(
  page: Page,
  getSessions: () => MockSession[],
  getOrdering: () => string[],
  onPut?: (order: string[]) => void,
) {
  await page.route("**/api/login/status", (r) =>
    r.fulfill({ json: { required: false, authenticated: true } }),
  );
  await page.route("**/api/sessions", (r) => {
    if (r.request().method() !== "GET") return r.fulfill({ status: 400 });
    return r.fulfill({
      json: {
        sessions: getSessions().map(sessionResponse),
        workspace_ordering: getOrdering(),
      },
    });
  });
  await page.route("**/api/workspace-ordering", (r) => {
    const body = JSON.parse(r.request().postData() || "{}") as {
      order?: string[];
    };
    if (body.order) onPut?.(body.order);
    return r.fulfill({ json: { order: body.order ?? [] } });
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

async function readWorkspaceTitles(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const rows = Array.from(
      document.querySelectorAll<HTMLAnchorElement>(
        "[data-testid='sidebar-session-row']",
      ),
    );
    return rows
      .map((a) => a.querySelector("[title]")?.getAttribute("title") ?? "")
      .filter(Boolean);
  });
}

const TOGGLE = "[data-testid='sidebar-sort-toggle']";

test.describe("Sidebar sort-mode toggle (#1418)", () => {
  test("default is manual; rows follow server workspace_ordering", async ({
    page,
  }) => {
    const sessions: MockSession[] = [
      {
        id: "s-old",
        title: "old-ws",
        project_path: "/tmp/repo",
        branch: "feature/old",
        created_at: "2025-01-01T00:00:00Z",
      },
      {
        id: "s-new",
        title: "new-ws",
        project_path: "/tmp/repo",
        branch: "feature/new",
        created_at: "2025-04-01T00:00:00Z",
        last_accessed_at: "2025-04-15T00:00:00Z",
      },
    ];
    // Server pins old-ws above new-ws. Manual mode honors this even
    // though new-ws has a fresher last_accessed_at.
    await mockApis(
      page,
      () => sessions,
      () => ["/tmp/repo::feature/old", "/tmp/repo::feature/new"],
    );
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");

    await expect(page.locator(TOGGLE)).toHaveAttribute(
      "data-sort-mode",
      "manual",
    );
    await expect(page.locator(TOGGLE)).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    await expect
      .poll(() => readWorkspaceTitles(page), { timeout: 8000 })
      .toEqual(["old-ws", "new-ws"]);
  });

  test("clicking toggle reorders by last-activity desc and persists", async ({
    page,
    context,
  }) => {
    const sessions: MockSession[] = [
      {
        id: "s-old",
        title: "old-ws",
        project_path: "/tmp/repo",
        branch: "feature/old",
        created_at: "2025-01-01T00:00:00Z",
      },
      {
        id: "s-new",
        title: "new-ws",
        project_path: "/tmp/repo",
        branch: "feature/new",
        created_at: "2025-04-01T00:00:00Z",
        last_accessed_at: "2025-04-15T00:00:00Z",
      },
    ];
    await mockApis(
      page,
      () => sessions,
      () => ["/tmp/repo::feature/old", "/tmp/repo::feature/new"],
    );
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");

    await expect
      .poll(() => readWorkspaceTitles(page), { timeout: 8000 })
      .toEqual(["old-ws", "new-ws"]);

    await page.locator(TOGGLE).click();
    await expect(page.locator(TOGGLE)).toHaveAttribute(
      "data-sort-mode",
      "lastActivity",
    );
    await expect(page.locator(TOGGLE)).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await expect
      .poll(() => readWorkspaceTitles(page), { timeout: 4000 })
      .toEqual(["new-ws", "old-ws"]);

    const stored = await page.evaluate(() =>
      window.localStorage.getItem("aoe-sidebar-sort-mode"),
    );
    expect(stored).toBe("lastActivity");

    // Reload: mode and order persist on first paint without another
    // toggle click. Use the same browser context so localStorage carries.
    await page.reload();
    await expect(page.locator(TOGGLE)).toHaveAttribute(
      "data-sort-mode",
      "lastActivity",
    );
    await expect
      .poll(() => readWorkspaceTitles(page), { timeout: 8000 })
      .toEqual(["new-ws", "old-ws"]);

    // Toggling back restores manual order.
    await page.locator(TOGGLE).click();
    await expect(page.locator(TOGGLE)).toHaveAttribute(
      "data-sort-mode",
      "manual",
    );
    await expect
      .poll(() => readWorkspaceTitles(page), { timeout: 4000 })
      .toEqual(["old-ws", "new-ws"]);
    // suppress unused-binding lint without changing the signature
    void context;
  });

  test("drag affordances are absent in last-activity mode", async ({ page }) => {
    const sessions: MockSession[] = [
      {
        id: "s1",
        title: "alpha",
        project_path: "/tmp/repo",
        branch: "feature/a",
        created_at: "2025-01-01T00:00:00Z",
      },
      {
        id: "s2",
        title: "beta",
        project_path: "/tmp/repo",
        branch: "feature/b",
        created_at: "2025-02-01T00:00:00Z",
      },
    ];
    const puts: string[][] = [];
    await mockApis(
      page,
      () => sessions,
      () => ["/tmp/repo::feature/a", "/tmp/repo::feature/b"],
      (order) => puts.push(order),
    );
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");

    // Sanity: drag wrappers are present in manual mode.
    await expect(
      page.locator("[aria-roledescription='Press and hold to reorder']"),
    ).toHaveCount(2, { timeout: 8000 });

    await page.locator(TOGGLE).click();
    await expect(page.locator(TOGGLE)).toHaveAttribute(
      "data-sort-mode",
      "lastActivity",
    );

    // Drag wrappers gone in last-activity mode.
    await expect(
      page.locator("[aria-roledescription='Press and hold to reorder']"),
    ).toHaveCount(0);

    // No PUT to workspace-ordering fires from a press-and-hold attempt.
    const rows = page.locator("[data-testid='sidebar-session-row']");
    const sourceBox = await rows.nth(1).boundingBox();
    if (!sourceBox) throw new Error("row missing");
    await page.mouse.move(
      sourceBox.x + sourceBox.width - 4,
      sourceBox.y + sourceBox.height / 2,
    );
    await page.mouse.down();
    await page.waitForTimeout(300);
    await page.mouse.move(sourceBox.x + 4, sourceBox.y + 4, { steps: 6 });
    await page.mouse.up();

    // Toggle back to manual: drag affordances return.
    await page.locator(TOGGLE).click();
    await expect(
      page.locator("[aria-roledescription='Press and hold to reorder']"),
    ).toHaveCount(2);

    expect(puts.length).toBe(0);
  });

  test("multi-repo group stays pinned at the bottom in last-activity mode", async ({
    page,
  }) => {
    const sessions: MockSession[] = [
      {
        id: "s-multi",
        title: "multi-recent",
        project_path: "/tmp/repo",
        branch: "feature/multi",
        // Highest last_accessed_at across the dataset; absent the pin
        // this would float to the top of the sidebar.
        created_at: "2025-01-01T00:00:00Z",
        last_accessed_at: "2025-12-01T00:00:00Z",
        workspace_repos: [
          { name: "repo-a", source_path: "/tmp/repo", branch: "feature/multi" },
          { name: "repo-b", source_path: "/tmp/other", branch: "feature/multi" },
        ],
      },
      {
        id: "s-single",
        title: "single-old",
        project_path: "/tmp/repo",
        branch: "feature/single",
        created_at: "2025-02-01T00:00:00Z",
        last_accessed_at: "2025-03-01T00:00:00Z",
      },
    ];
    await mockApis(
      page,
      () => sessions,
      () => [
        "/tmp/repo::feature/multi",
        "/tmp/repo::feature/single",
      ],
    );
    await page.setViewportSize({ width: 1280, height: 720 });
    await page.goto("/");

    // Wait for the sidebar to render both rows before flipping the mode.
    await expect(
      page.locator("[data-testid='sidebar-session-row']"),
    ).toHaveCount(2, { timeout: 8000 });

    await page.locator(TOGGLE).click();
    await expect(page.locator(TOGGLE)).toHaveAttribute(
      "data-sort-mode",
      "lastActivity",
    );

    // Multi-repo group's row stays last even though its workspace has
    // the freshest activity. Single-repo row renders first.
    await expect
      .poll(() => readWorkspaceTitles(page), { timeout: 4000 })
      .toEqual(["single-old", "multi-recent"]);
  });
});
