// User story: the red "Cockpit agent failed to start" banner shows
// the new native-binary-launch-failure remediation and exposes an
// Open agent log affordance that round-trips the worker-log endpoint.
//
// The fake ACP agent's `failOn` script field rejects `session/new`
// with a JSON-RPC error whose `data.details` matches the native-binary
// regex; cockpit's spawn path turns that into an AgentStartupError
// event the React banner reads. See #1449.

import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test as base, expect } from "@playwright/test";
import {
  spawnAoeServe,
  listSessions,
  seedSessionViaAoeAdd,
} from "../../helpers/aoeServe";

const NATIVE_BINARY_DETAILS =
  "Claude Code native binary at /usr/lib/node_modules/@agentclientprotocol/claude-agent-acp/node_modules/@anthropic-ai/claude-agent-sdk-linux-arm64/claude exists but failed to launch.";

const SCRIPT = {
  failOn: {
    method: "session/new",
    code: -32603,
    message: "Internal error",
    data: { details: NATIVE_BINARY_DETAILS },
  },
};

interface ReplayFrameEvent {
  AgentStartupError?: { message?: string };
}

async function pollForStartupError(
  baseUrl: string,
  sessionId: string,
  timeoutMs = 20_000,
): Promise<string> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const res = await fetch(
      `${baseUrl}/api/sessions/${sessionId}/cockpit/replay?since=0`,
    );
    if (res.ok) {
      const body = (await res.json()) as {
        frames?: Array<{ event?: ReplayFrameEvent }>;
      };
      for (const f of body.frames ?? []) {
        const msg = f.event?.AgentStartupError?.message;
        if (typeof msg === "string" && msg.length > 0) return msg;
      }
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error("never observed AgentStartupError on replay");
}

base("startup banner: native-binary branch + agent-log disclosure", async ({ page }, testInfo) => {
  const scriptDir = mkdtempSync(join(tmpdir(), "aoe-pw-native-binary-"));
  const scriptPath = join(scriptDir, "script.json");
  writeFileSync(scriptPath, JSON.stringify(SCRIPT));

  let serve: Awaited<ReturnType<typeof spawnAoeServe>> | undefined;

  try {
    serve = await spawnAoeServe({
      authMode: "none",
      cockpit: true,
      fakeAcpScript: scriptPath,
      workerIndex: testInfo.workerIndex,
      parallelIndex: testInfo.parallelIndex,
      seedFn: seedSessionViaAoeAdd({ title: "story-native-binary" }),
    });

    const sessions = await listSessions(serve.baseUrl);
    const seeded = sessions.find((s) => s.title === "story-native-binary");
    if (!seeded) throw new Error("seeded session 'story-native-binary' missing");
    const sessionId = seeded.id;

    // Bypass enableCockpitAndWait: it throws when an AgentStartupError
    // shows up on replay, which is exactly the state this spec is
    // trying to land in. Hit the enable endpoint directly and then
    // poll for the typed error event ourselves.
    const enableRes = await fetch(
      `${serve.baseUrl}/api/sessions/${sessionId}/cockpit/enable`,
      { method: "POST" },
    );
    expect(enableRes.ok).toBe(true);
    const msg = await pollForStartupError(serve.baseUrl, sessionId);
    expect(msg).toContain("native binary");
    expect(msg).toContain("failed to launch");

    await page.goto(`${serve.baseUrl}/session/${encodeURIComponent(sessionId)}`);

    // Banner is in the cockpit chrome above the composer. Match on the
    // header text plus a piece of the new remediation copy so a future
    // rewording of either alone surfaces here.
    const banner = page.getByText("Cockpit agent failed to start");
    await expect(banner).toBeVisible({ timeout: 15_000 });
    await expect(page.getByText(/Architecture mismatch/i)).toBeVisible();
    await expect(page.getByText(/aoe cockpit doctor --fix/)).toHaveCount(0);

    const toggle = page.getByTestId("cockpit-agent-log-toggle");
    await expect(toggle).toBeVisible();
    await toggle.click();

    // Either a populated <pre> or the "no log output yet" placeholder
    // is an acceptable terminal state: the runner may have written
    // adapter stderr to the log before exit, or it may have exited
    // before flushing. Both confirm the disclosure round-tripped the
    // endpoint.
    const pre = page.getByTestId("cockpit-agent-log-pre");
    const empty = page.getByText("No log output yet");
    await expect(pre.or(empty)).toBeVisible({ timeout: 10_000 });

    // Refresh re-issues the GET. The visible state should remain
    // consistent (still pre or still empty).
    await page.getByTestId("cockpit-agent-log-refresh").click();
    await expect(pre.or(empty)).toBeVisible({ timeout: 5_000 });
  } finally {
    try {
      if (serve) await serve.stop();
    } finally {
      rmSync(scriptDir, { recursive: true, force: true });
    }
  }
});
