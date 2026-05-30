// Story #3 (#1515): expanding the Cockpit "Advanced" fold and editing a knob
// inside it persists through the same save-on-change path as any other field.
//
// Drives the real settings UI (not the REST endpoint): land on
// /settings/cockpit, confirm the advanced knobs are folded away by default,
// expand the fold, edit cockpit.replay_bytes, and assert the value reaches the
// profile config and survives a page reload.

import { test, expect } from "../helpers/liveTest";

test("cockpit advanced knob edits persist after expanding the fold", async ({
  serve,
  page,
}) => {
  const profiles: Array<{ name: string; is_default?: boolean }> = await fetch(
    `${serve.baseUrl}/api/profiles`,
  ).then((r) => r.json());
  const defaultProfile =
    profiles.find((p) => p.is_default)?.name ?? profiles[0]?.name ?? "main";
  const profileUrl = `${serve.baseUrl}/api/profiles/${encodeURIComponent(defaultProfile)}/settings`;

  const before = await fetch(profileUrl).then((r) => r.json());
  const baseline = (before?.cockpit?.replay_bytes as number | undefined) ?? 0;
  const newValue = baseline === 4096 ? 8192 : 4096;

  await page.goto(`${serve.baseUrl}/settings/cockpit`);

  // The high-level master switch is visible immediately; the advanced knob is
  // folded away by default.
  await expect(page.getByText("Cockpit master switch")).toBeVisible({
    timeout: 10_000,
  });
  await expect(page.getByText("Replay buffer bytes")).toHaveCount(0);

  // Expand the Advanced fold.
  await page.getByRole("button", { name: /Advanced/ }).first().click();

  const replayInput = page
    .locator("label", { hasText: /^Replay buffer bytes$/ })
    .locator("..")
    .locator('input[type="number"]');
  await expect(replayInput).toBeVisible({ timeout: 5_000 });

  // Edit and commit (NumberField commits on blur / Enter).
  await replayInput.fill(String(newValue));
  await replayInput.press("Enter");

  // Server-side: PATCH landed against the profile config.
  await expect(async () => {
    const after = await fetch(profileUrl).then((r) => r.json());
    expect(after?.cockpit?.replay_bytes).toBe(newValue);
  }).toPass({ timeout: 5_000 });

  // Frontend-side: after reload the fold is collapsed again (component-local,
  // not persisted), and re-expanding shows the persisted value.
  await page.reload();
  await expect(page.getByText("Cockpit master switch")).toBeVisible({
    timeout: 10_000,
  });
  await expect(page.getByText("Replay buffer bytes")).toHaveCount(0);

  await page.getByRole("button", { name: /Advanced/ }).first().click();
  await expect(replayInput).toHaveValue(String(newValue), { timeout: 5_000 });
});
