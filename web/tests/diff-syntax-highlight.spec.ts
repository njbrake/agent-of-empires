import { test, expect } from "@playwright/test";
import { mockTerminalApis } from "./helpers/terminal-mocks";
import type { Page } from "@playwright/test";

const DIFF_FILES_RESPONSE = {
  files: [
    {
      path: "src/example.ts",
      old_path: null,
      status: "modified",
      additions: 3,
      deletions: 1,
    },
  ],
  base_branch: "main",
  warning: null,
};

const DIFF_FILE_RESPONSE = {
  file: {
    path: "src/example.ts",
    old_path: null,
    status: "modified",
    additions: 3,
    deletions: 1,
  },
  hunks: [
    {
      old_start: 1,
      old_lines: 3,
      new_start: 1,
      new_lines: 5,
      lines: [
        {
          type: "equal",
          old_line_num: 1,
          new_line_num: 1,
          content: 'import { useState } from "react";\n',
        },
        {
          type: "delete",
          old_line_num: 2,
          new_line_num: null,
          content: "const x = 42;\n",
        },
        {
          type: "add",
          old_line_num: null,
          new_line_num: 2,
          content: "const x: number = 42;\n",
        },
        {
          type: "add",
          old_line_num: null,
          new_line_num: 3,
          content: "function greet(name: string): string {\n",
        },
        {
          type: "add",
          old_line_num: null,
          new_line_num: 4,
          content: "  return `Hello, ${name}`;\n",
        },
        {
          type: "equal",
          old_line_num: 3,
          new_line_num: 5,
          content: "export default x;\n",
        },
      ],
    },
  ],
  is_binary: false,
  truncated: false,
};

/** Set up API mocks with real diff data for syntax highlighting tests. */
async function setupDiffMocks(page: Page) {
  await mockTerminalApis(page);
  // Override the diff/files endpoint from mockTerminalApis (which returns empty).
  await page.route("**/api/sessions/*/diff/files", (r) =>
    r.fulfill({ json: DIFF_FILES_RESPONSE }),
  );
  await page.route(/\/api\/sessions\/[^/]+\/diff\/file\?/, (r) =>
    r.fulfill({ json: DIFF_FILE_RESPONSE }),
  );
}

/** Open a session and wait for the diff file list to load. */
async function openSessionAndWaitForDiffList(page: Page) {
  await expect(page.locator("header")).toBeVisible();
  await page.locator('button:has-text("pinch-test")').nth(1).click();
  // Wait for the file entry to appear in the diff panel (API fetch + render).
  await expect(page.getByText("example.ts").first()).toBeVisible({
    timeout: 10000,
  });
}

test.use({ viewport: { width: 1280, height: 720 } });

test.describe("Diff syntax highlighting", () => {
  test("renders syntax-highlighted tokens for TypeScript diffs", async ({
    page,
  }) => {
    await setupDiffMocks(page);
    await page.goto("/");
    await openSessionAndWaitForDiffList(page);

    // Click the TypeScript file.
    await page.getByText("example.ts").first().click();

    // Wait for syntax-highlighted tokens: spans with an inline `color` style
    // inside the diff content area.
    const highlightedSpan = page.locator(
      ".leading-\\[1\\.6\\] span[style*='color']",
    );
    await expect(highlightedSpan.first()).toBeVisible({ timeout: 15000 });

    // Multiple tokens should be rendered (keywords, identifiers, strings, etc.).
    const count = await highlightedSpan.count();
    expect(count).toBeGreaterThan(3);
  });

  test("preserves diff +/- prefix markers alongside syntax tokens", async ({
    page,
  }) => {
    await setupDiffMocks(page);
    await page.goto("/");
    await openSessionAndWaitForDiffList(page);

    await page.getByText("example.ts").first().click();

    // Wait for highlighting to load.
    await expect(
      page.locator(".leading-\\[1\\.6\\] span[style*='color']").first(),
    ).toBeVisible({ timeout: 15000 });

    // The "+" prefix spans should still exist for added lines.
    const plusPrefixes = page
      .locator(".leading-\\[1\\.6\\]")
      .getByText("+", { exact: true });
    expect(await plusPrefixes.count()).toBeGreaterThanOrEqual(3);

    // The "-" prefix span should still exist for the deleted line.
    const minusPrefixes = page
      .locator(".leading-\\[1\\.6\\]")
      .getByText("-", { exact: true });
    expect(await minusPrefixes.count()).toBeGreaterThanOrEqual(1);
  });

  test("falls back to plain text for unrecognised extensions", async ({
    page,
  }) => {
    await mockTerminalApis(page);
    // Override diff endpoints with an unrecognised file type.
    await page.route("**/api/sessions/*/diff/files", (r) =>
      r.fulfill({
        json: {
          files: [
            {
              path: "data.xyz",
              old_path: null,
              status: "added",
              additions: 1,
              deletions: 0,
            },
          ],
          base_branch: "main",
          warning: null,
        },
      }),
    );
    await page.route(/\/api\/sessions\/[^/]+\/diff\/file\?/, (r) =>
      r.fulfill({
        json: {
          file: {
            path: "data.xyz",
            old_path: null,
            status: "added",
            additions: 1,
            deletions: 0,
          },
          hunks: [
            {
              old_start: 0,
              old_lines: 0,
              new_start: 1,
              new_lines: 1,
              lines: [
                {
                  type: "add",
                  old_line_num: null,
                  new_line_num: 1,
                  content: "some unknown format content\n",
                },
              ],
            },
          ],
          is_binary: false,
          truncated: false,
        },
      }),
    );

    await page.goto("/");
    await expect(page.locator("header")).toBeVisible();
    await page.locator('button:has-text("pinch-test")').nth(1).click();
    await expect(page.getByText("data.xyz").first()).toBeVisible({
      timeout: 10000,
    });

    await page.getByText("data.xyz").first().click();

    // The line content should render as plain text.
    const contentLine = page.getByText("some unknown format content");
    await expect(contentLine).toBeVisible({ timeout: 5000 });

    // No syntax-highlighted spans should be present.
    const highlightedSpans = page.locator(
      ".leading-\\[1\\.6\\] span[style*='color']",
    );
    await expect(highlightedSpans).toHaveCount(0);
  });
});
