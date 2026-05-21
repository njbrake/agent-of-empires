import { expect } from "@playwright/test";

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
 * This helper polls the disk-backed replay endpoint until any frame
 * appears, indicating the supervisor finished its handshake and is
 * emitting events. After this resolves, sending a prompt is safe.
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
}
