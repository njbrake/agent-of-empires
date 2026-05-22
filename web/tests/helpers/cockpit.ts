import { expect, type Locator, type Page } from "@playwright/test";

/**
 * Wait for the cockpit supervisor to be ready to accept prompts after a
 * `POST /api/sessions/<id>/cockpit/enable`.
 *
 * `cockpit/enable` is fire-and-forget: it flips the per-session
 * `cockpit_mode` flag and `tokio::spawn`s the supervisor, returning
 * before the ACP `initialize` + `session/new` handshake completes.
 * Sending a `/cockpit/prompt` immediately races that handshake and the
 * prompt can be lost or queued unpredictably, which surfaces as flaky
 * test failures (notably `cockpit-approval.spec.ts` and
 * `cockpit-spawn-prompt.spec.ts` used to do this with a hardcoded
 * `setTimeout(2_000)` that proved tight under CI load).
 *
 * Two-phase wait:
 *   1. Poll `/cockpit/replay` until any frame appears (proves the
 *      worker process is up and the supervisor finished `initialize`).
 *   2. Poll `/api/sessions` until the seeded session reports
 *      `cockpit_worker_state === "running"` (proves the ACP
 *      `session/new` handshake completed and the supervisor is ready
 *      to forward prompts).
 *
 * Without phase 2 the React client receives `cockpit_worker_state ===
 * "resuming"` from the initial `/api/sessions` fetch on navigation,
 * the `useCockpit` drain effect parks the prompt, and the test races
 * the next REST poll cycle (a few seconds at best, indefinitely under
 * CI load) before "Send a message" Enter actually reaches the
 * supervisor.
 *
 * Default timeout is 15s; the local happy-path completes in well under
 * 1s, so 15s only kicks in on contended CI runners.
 */
export async function waitForCockpitReady(
  baseUrl: string,
  sessionId: string,
  timeoutMs = 15_000,
): Promise<void> {
  await expect
    .poll(
      async () => {
        const replay = await fetch(
          `${baseUrl}/api/sessions/${sessionId}/cockpit/replay?since=0`,
        ).then((r) => r.json());
        const frames: unknown[] = Array.isArray(replay)
          ? replay
          : (replay.frames ?? []);
        return frames.length;
      },
      { timeout: timeoutMs, intervals: [100, 200, 200, 200] },
    )
    .toBeGreaterThan(0);
  // Phase 2: wait for worker to reach "running".
  await expect
    .poll(
      async () => {
        const res = await fetch(`${baseUrl}/api/sessions`);
        if (!res.ok) return "fetch-failed";
        const body = await res.json();
        const sessions: Array<{ id: string; cockpit_worker_state?: string }> = Array.isArray(
          body,
        )
          ? body
          : (body.sessions ?? []);
        const me = sessions.find((s) => s.id === sessionId);
        return me?.cockpit_worker_state ?? "absent";
      },
      { timeout: timeoutMs, intervals: [100, 200, 200, 500] },
    )
    .toBe("running");
}

/**
 * Enable the cockpit supervisor for a session and wait for it to be
 * ready to accept prompts. Asserts the enable POST succeeded before
 * polling readiness so a 4xx / 5xx surfaces as an explicit failure
 * rather than a readiness timeout.
 */
export async function enableCockpitAndWait(
  baseUrl: string,
  sessionId: string,
  timeoutMs = 15_000,
): Promise<void> {
  const res = await fetch(
    `${baseUrl}/api/sessions/${sessionId}/cockpit/enable`,
    { method: "POST" },
  );
  if (!res.ok) {
    throw new Error(
      `cockpit enable failed: status=${res.status} body=${await res.text()}`,
    );
  }
  await waitForCockpitReady(baseUrl, sessionId, timeoutMs);
}

/**
 * Wait for the cockpit React surface to be mounted and interactive on
 * the current page. Resolves when the composer textarea is visible.
 *
 * `waitForCockpitReady` checks the server-side supervisor handshake via
 * disk-backed replay; this checks the client side after `page.goto`. UI
 * story specs need both: the supervisor must be alive (otherwise prompt
 * sends drop), and the React tree must have mounted CockpitView so
 * clicks have something to land on.
 *
 * Default timeout is 15s, matching `waitForCockpitReady`; the textbox
 * appears within a few hundred ms on the local happy path. Lazy-loaded
 * cockpit chunks (`App.tsx` dynamic `import("./components/cockpit/CockpitView")`)
 * may add a short delay on first navigation.
 */
export async function waitForCockpitView(
  page: Page,
  timeoutMs = 15_000,
): Promise<void> {
  await expect(
    page.getByRole("textbox", {
      name: /Send a message|Queue a follow-up/i,
    }),
  ).toBeVisible({ timeout: timeoutMs });
}

/**
 * Resolve the `<select>` rendered by a FormFields `SelectField` whose
 * label text matches the given string. SelectField wraps `<label>` and
 * `<select>` as siblings inside a single `<div>`; the cheapest
 * unambiguous selector walks from the label up to that wrapper.
 */
export function settingsSelectByLabel(page: Page, labelText: string): Locator {
  return page
    .locator("label")
    .filter({ hasText: labelText })
    .locator("xpath=..")
    .locator("select")
    .first();
}

/**
 * Resolve the `<input type="number">` rendered by NumberField whose
 * label text matches the given string. Same structure as
 * settingsSelectByLabel.
 */
export function settingsNumberInputByLabel(page: Page, labelText: string): Locator {
  return page
    .locator("label")
    .filter({ hasText: labelText })
    .locator("xpath=..")
    .locator('input[type="number"]')
    .first();
}

/** Click a top-level SettingsView tab by its visible label. */
export async function openSettingsTab(page: Page, label: string): Promise<void> {
  await page.getByRole("button", { name: label, exact: true }).click();
}

/**
 * Locator scoped to the active SessionWizard dialog. The wizard renders
 * a fixed-position overlay that visually covers the sidebar; without
 * this scope, `getByRole`/`getByText` queries can match background
 * sidebar / topbar elements behind the overlay.
 */
export function wizardScope(page: Page): Locator {
  return page.locator(
    'div.fixed.inset-0.z-50:has(h1:has-text("New session"))',
  );
}

/**
 * Resolve the first text input that sits in the same wrapper `<div>`
 * as a `<label>` matching the given text. Works for both the wizard's
 * SessionStep field layout and FormFields TextField.
 */
export function inputByLabel(page: Page, labelText: string): Locator {
  return page
    .locator("label")
    .filter({ hasText: labelText })
    .locator("xpath=..")
    .locator('input[type="text"]')
    .first();
}
