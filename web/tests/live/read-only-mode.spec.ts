// Read-only mode coverage.
//
// `aoe serve --read-only` must (1) advertise read_only=true on /api/about
// so the frontend can suppress mutation UI, and (2) reject mutating HTTP
// requests with 403. Both layers must hold; src/server/tests/read_only.rs
// (Rust side) does the full mutation-endpoint table.

import { test, expect } from "../helpers/liveTest";

test("/api/about reports read_only=true", async ({ serveReadOnly }) => {
  const about = await fetch(`${serveReadOnly.baseUrl}/api/about`).then((r) =>
    r.json(),
  );
  expect(about?.read_only).toBe(true);
});

test("POST /api/sessions is rejected with 403", async ({ serveReadOnly }) => {
  // Body shape matches CreateSessionBody (path, tool, ...). The read-only
  // guard sits AFTER axum's Json<...> extractor today, so a bad-shape
  // body would 422 before the guard fires. The right-shape body trips
  // the guard at the top of `create_session` and returns 403.
  //
  // Moving the read-only check to precede body validation is tracked as
  // a follow-up; the test contract here is "POST with a valid shape on a
  // read-only server returns 403", not "any POST returns 403".
  const res = await fetch(`${serveReadOnly.baseUrl}/api/sessions`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      title: "blocked",
      path: "/tmp/whatever",
      tool: "claude",
    }),
  });
  expect(res.status).toBe(403);
});

test("dashboard suppresses mutation UI in read-only", async ({
  serveReadOnly,
  page,
}) => {
  await page.goto(serveReadOnly.baseUrl);

  // The dashboard's empty state would normally invite the user to create
  // a new session. In read-only mode, the "n" keyboard shortcut handler
  // must not open the wizard. Press it and assert the wizard heading
  // never appears.
  await page.locator("body").click();
  await page.keyboard.press("n");
  await expect(
    page.getByRole("heading", { name: "New session" }),
  ).toBeHidden({ timeout: 2_000 });
});
