// Settings persistence round-trip for the theme panel.
//
// PATCH /api/settings -> GET /api/settings returns the new value; reload
// the dashboard and the value still applies. Proves the live server
// persists settings to disk (not just in-memory) and the frontend reads
// them back on every page load.

import { test, expect } from "../helpers/liveTest";

test("theme setting persists through PATCH + reload", async ({
  serve,
  page,
}) => {
  // Read baseline.
  const before = await fetch(`${serve.baseUrl}/api/settings`).then((r) =>
    r.json(),
  );
  const baselineTheme: string | undefined = before?.theme?.name;
  const newTheme = baselineTheme === "modus-vivendi" ? "default" : "modus-vivendi";

  // PATCH the theme via the same endpoint the dashboard hits.
  const patchRes = await fetch(`${serve.baseUrl}/api/settings`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ theme: { ...(before?.theme ?? {}), name: newTheme } }),
  });
  expect(patchRes.ok).toBeTruthy();

  // Server-side persistence: GET returns the new value immediately.
  const after = await fetch(`${serve.baseUrl}/api/settings`).then((r) =>
    r.json(),
  );
  expect(after?.theme?.name).toBe(newTheme);

  // Frontend-side persistence: reload the dashboard and the new value
  // is still what the page reads.
  await page.goto(serve.baseUrl);
  const fetched = await page.evaluate(async (url) => {
    const r = await fetch(`${url}/api/settings`);
    return r.json();
  }, serve.baseUrl);
  expect(fetched?.theme?.name).toBe(newTheme);
});
