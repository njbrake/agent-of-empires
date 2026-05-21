// GET /api/devices coverage.
//
// `ConnectedDevices.tsx` lists devices that have authenticated against
// the server. In `--no-auth` mode the middleware bypasses
// `record_device` entirely (src/server/auth.rs:530-553), so this spec
// uses `serveToken` where every successful token-authenticated request
// hits `record_device` (src/server/auth.rs:697-703).
//
// Three flows:
//   1. Endpoint shape: after browser navigation, GET /api/devices
//      returns at least one entry with the documented fields.
//   2. Multi-device: a second authenticated request from a custom
//      User-Agent grows the list to two entries.
//   3. UI: the /settings/devices panel mounts ConnectedDevices and
//      renders the captured browser device.
//
// The issue (#1222) mentions "revoke path if an endpoint exists" --
// no such endpoint exists in the server today, so the multi-device
// flow asserts only the GET shape, per the issue note.

import { test, expect } from "../helpers/liveTest";

const DEVICE_FIELDS = [
  "ip",
  "user_agent",
  "first_seen",
  "last_seen",
  "request_count",
] as const;

test("GET /api/devices returns the navigating browser after token auth", async ({
  serveToken,
  page,
}) => {
  const token = serveToken.authToken!;
  expect(token, "harness exposes a token in token mode").toBeTruthy();

  await page.goto(`${serveToken.baseUrl}/?token=${token}`, {
    waitUntil: "domcontentloaded",
  });

  // Reach into the page context's cookie jar so the GET below carries
  // the same aoe_token the SPA persisted. Asserting via page.request
  // mirrors what the dashboard does after navigation.
  await expect
    .poll(
      async () => {
        const res = await page.request.get(`${serveToken.baseUrl}/api/devices`);
        if (!res.ok()) return -1;
        const list = (await res.json()) as unknown[];
        return list.length;
      },
      { timeout: 10_000, message: "browser device should be recorded" },
    )
    .toBeGreaterThan(0);

  const res = await page.request.get(`${serveToken.baseUrl}/api/devices`);
  expect(res.ok()).toBeTruthy();
  const list = (await res.json()) as Array<Record<string, unknown>>;

  expect(list.length).toBeGreaterThan(0);
  const browserDevice = list[0]!;
  for (const field of DEVICE_FIELDS) {
    expect(browserDevice, `device entry has '${field}'`).toHaveProperty(field);
  }
  expect(typeof browserDevice.ip).toBe("string");
  expect(typeof browserDevice.user_agent).toBe("string");
  expect(typeof browserDevice.first_seen).toBe("string");
  expect(typeof browserDevice.last_seen).toBe("string");
  expect(typeof browserDevice.request_count).toBe("number");
  expect(browserDevice.request_count as number).toBeGreaterThan(0);
});

test("multi-device: a second authenticated UA appears as a distinct entry", async ({
  serveToken,
  page,
}) => {
  const token = serveToken.authToken!;
  await page.goto(`${serveToken.baseUrl}/?token=${token}`, {
    waitUntil: "domcontentloaded",
  });

  await expect
    .poll(async () => {
      const r = await page.request.get(`${serveToken.baseUrl}/api/devices`);
      if (!r.ok()) return 0;
      return ((await r.json()) as unknown[]).length;
    }, { timeout: 10_000 })
    .toBeGreaterThan(0);

  // Hit any token-gated endpoint with a distinct UA via Bearer auth.
  // record_device keys on (ip, user_agent), so the curl-like UA lands
  // as a new entry even though the IP is the same loopback.
  const curlUa = "aoe-e2e-fake-curl/1.0";
  const second = await fetch(`${serveToken.baseUrl}/api/about`, {
    headers: {
      Authorization: `Bearer ${token}`,
      "User-Agent": curlUa,
    },
  });
  expect(second.status).toBe(200);

  await expect
    .poll(
      async () => {
        const r = await page.request.get(`${serveToken.baseUrl}/api/devices`);
        if (!r.ok()) return [];
        const list = (await r.json()) as Array<{ user_agent: string }>;
        return list.map((d) => d.user_agent);
      },
      { timeout: 10_000, message: "second UA should land as a new device row" },
    )
    .toContain(curlUa);

  const final = await page.request.get(`${serveToken.baseUrl}/api/devices`);
  const list = (await final.json()) as Array<{ user_agent: string }>;
  expect(list.length).toBeGreaterThanOrEqual(2);
});

test("settings -> devices renders ConnectedDevices with captured browser", async ({
  serveToken,
  page,
}) => {
  const token = serveToken.authToken!;
  await page.goto(`${serveToken.baseUrl}/?token=${token}`, {
    waitUntil: "domcontentloaded",
  });

  // Wait for the dashboard to finish bootstrap. /api/login/status
  // fires once on load and the dashboard is gated on its 200.
  await expect
    .poll(
      async () => {
        const r = await page.request.get(`${serveToken.baseUrl}/api/devices`);
        if (!r.ok()) return 0;
        return ((await r.json()) as unknown[]).length;
      },
      { timeout: 10_000 },
    )
    .toBeGreaterThan(0);

  await page.goto(`${serveToken.baseUrl}/settings/devices`, {
    waitUntil: "domcontentloaded",
  });

  // Heading text is uppercased via Tailwind but the underlying DOM
  // node is "Connected Devices" (see ConnectedDevices.tsx).
  await expect(
    page.getByRole("heading", { name: /connected devices/i }),
  ).toBeVisible({ timeout: 10_000 });

  // The list renders one row per (ip, user_agent). The loopback IP
  // varies by host stack: 127.0.0.1 on most Linux/macOS setups, ::1
  // where IPv6 wins the bind. Match either so the spec stays portable.
  await expect(page.getByText(/(127\.0\.0\.1|::1)/).first()).toBeVisible({
    timeout: 10_000,
  });
});
