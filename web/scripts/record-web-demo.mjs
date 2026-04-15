#!/usr/bin/env node
// Records two GIFs of the web dashboard:
//   docs/assets/web-desktop.gif  (1280x720 viewport)
//   docs/assets/web-mobile.gif   (iPhone 13)
//
// Spawns an isolated `aoe serve --no-auth` with a couple of demo sessions
// running bash, drives Playwright's chromium against it, records WebM,
// then converts to GIF with ffmpeg's palette filter.
//
// Usage:
//   cargo build --release --features serve
//   node web/scripts/record-web-demo.mjs
//
// Requires: ffmpeg, chromium (Playwright-bundled is fine).

import { chromium, devices } from "@playwright/test";
import { spawn, spawnSync } from "node:child_process";
import {
  mkdtempSync,
  mkdirSync,
  rmSync,
  existsSync,
  readdirSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import net from "node:net";

const __dirname = dirname(fileURLToPath(import.meta.url));
// __dirname = web/scripts; repo root is two levels up.
const repo = resolve(__dirname, "..", "..");
const aoe = join(repo, "target/release/aoe");
const outDir = join(repo, "docs/assets");

function freePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.unref();
    srv.on("error", reject);
    srv.listen(0, () => {
      const addr = srv.address();
      if (typeof addr === "object" && addr) {
        const port = addr.port;
        srv.close(() => resolve(port));
      } else {
        reject(new Error("no port"));
      }
    });
  });
}

async function waitForServer(url, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(`${url}/api/sessions`);
      if (r.ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`aoe serve did not become ready within ${timeoutMs}ms`);
}

function webmToGif(webmPath, gifPath, width) {
  const filter = [
    `[0:v] fps=15,scale=${width}:-1:flags=lanczos,split [a][b];`,
    `[a] palettegen=max_colors=128 [p];`,
    `[b][p] paletteuse=dither=bayer:bayer_scale=5`,
  ].join(" ");
  const r = spawnSync(
    "ffmpeg",
    ["-y", "-i", webmPath, "-filter_complex", filter, gifPath],
    { stdio: "inherit" },
  );
  if (r.status !== 0) throw new Error(`ffmpeg failed: ${r.status}`);
}

async function record({ browser, url, outGif, viewport, device, script, width }) {
  const videoDir = mkdtempSync(join(tmpdir(), "aoe-vid-"));
  const recordSize = device ? device.viewport : viewport;
  const contextOpts = device
    ? { ...device, recordVideo: { dir: videoDir, size: recordSize } }
    : { viewport, recordVideo: { dir: videoDir, size: recordSize } };
  const ctx = await browser.newContext(contextOpts);
  const page = await ctx.newPage();
  await page.goto(url);
  await script(page);
  await page.close();
  await ctx.close();
  const webm = readdirSync(videoDir).find((f) => f.endsWith(".webm"));
  if (!webm) throw new Error(`no webm produced in ${videoDir}`);
  webmToGif(join(videoDir, webm), outGif, width);
  rmSync(videoDir, { recursive: true, force: true });
  console.log(`wrote ${outGif}`);
}

async function desktopScript(page) {
  await page.waitForLoadState("networkidle");
  await page.waitForTimeout(800);
  // Sidebar shows the two sessions. Click the "Shell" one.
  const shellBtn = page.locator("button").filter({ hasText: "Shell" }).first();
  await shellBtn.click();
  await page.waitForSelector(".xterm", { timeout: 10_000 });
  await page.waitForTimeout(1500);
  // Focus the xterm and type some commands.
  await page.locator(".xterm").first().click();
  await page.waitForTimeout(300);
  await page.keyboard.type("ls -la", { delay: 40 });
  await page.waitForTimeout(400);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(700);
  await page.keyboard.type('echo "Agent of Empires — web dashboard"', {
    delay: 35,
  });
  await page.waitForTimeout(250);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(900);
  await page.keyboard.type("for i in 1 2 3; do printf 'step %d\\n' $i; sleep 0.3; done", {
    delay: 25,
  });
  await page.waitForTimeout(250);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(2500);
  // Hero moment: show xterm rendering one more command with colored output.
  await page.keyboard.type("ls --color=always -la /etc | head -10", { delay: 25 });
  await page.waitForTimeout(250);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(1800);
}

async function mobileScript(page) {
  await page.waitForLoadState("networkidle");
  await page.waitForTimeout(1000);
  const shellBtn = page.locator("button").filter({ hasText: "Shell" }).first();
  await shellBtn.click();
  await page.waitForSelector(".xterm", { timeout: 10_000 });
  await page.waitForTimeout(1800);
  await page.locator(".xterm").first().tap();
  await page.waitForTimeout(400);
  await page.keyboard.type('echo "hello from my phone"', { delay: 50 });
  await page.waitForTimeout(300);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(1200);
  await page.keyboard.type("ls", { delay: 70 });
  await page.waitForTimeout(200);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(1800);
}

async function main() {
  if (!existsSync(aoe)) {
    console.error(`missing ${aoe}; run: cargo build --release --features serve`);
    process.exit(1);
  }

  const sandbox = mkdtempSync(join(tmpdir(), "aoe-demo-"));
  const home = join(sandbox, "home");
  const xdg = join(home, ".config");
  const bin = join(sandbox, "bin");
  const proj = join(sandbox, "demo-project");
  mkdirSync(home);
  mkdirSync(xdg, { recursive: true });
  mkdirSync(bin);
  mkdirSync(proj);

  // Shim: aoe only accepts a known tool name as --cmd, and we want a live
  // bash in the pane for the demo. A script named `claude` on PATH that
  // execs bash satisfies aoe's detection without calling the real agent.
  const fs = await import("node:fs");
  const shim = join(bin, "claude");
  fs.writeFileSync(
    shim,
    '#!/bin/bash\nexec bash --noprofile --norc -i\n',
  );
  fs.chmodSync(shim, 0o755);

  spawnSync("git", ["init", "-q"], { cwd: proj });
  spawnSync(
    "git",
    ["commit", "--allow-empty", "-q", "-m", "init"],
    {
      cwd: proj,
      env: {
        ...process.env,
        GIT_AUTHOR_NAME: "d",
        GIT_AUTHOR_EMAIL: "d@d",
        GIT_COMMITTER_NAME: "d",
        GIT_COMMITTER_EMAIL: "d@d",
      },
    },
  );

  const env = {
    ...process.env,
    HOME: home,
    XDG_CONFIG_HOME: xdg,
    // PATH must NOT include the user's real Claude Code install — the shim
    // `claude` must be the only one resolvable, so bash runs in the pane
    // instead of the real agent (no API credits burned, no trust dialog).
    PATH: `${bin}:/usr/local/bin:/usr/bin:/bin`,
  };

  // --cmd-override forces aoe to exec our shim regardless of what else is
  // on PATH. The "-c claude" picks the Claude Code tool preset for chrome
  // (pane label, icon), but the binary that actually runs is our bash shim.
  const addRes = spawnSync(
    aoe,
    ["add", proj, "-t", "Shell", "-c", "claude", "--cmd-override", shim],
    { env },
  );
  if (addRes.status !== 0) {
    throw new Error(`aoe add failed: ${addRes.stderr?.toString() ?? ""}`);
  }

  const port = await freePort();
  const url = `http://127.0.0.1:${port}`;
  const server = spawn(
    aoe,
    ["serve", "--host", "127.0.0.1", "--port", String(port), "--no-auth"],
    { env, stdio: "pipe" },
  );
  server.stderr.on("data", (d) => process.stderr.write(`[aoe] ${d}`));

  try {
    await waitForServer(url, 10_000);

    // Launch each session so it has a live tmux pane before we record.
    const sessions = await fetch(`${url}/api/sessions`).then((r) => r.json());
    for (const s of sessions) {
      await fetch(`${url}/api/sessions/${s.id}/ensure`, { method: "POST" });
    }
    await new Promise((r) => setTimeout(r, 800));

    const browser = await chromium.launch({ args: ["--no-sandbox"] });

    await record({
      browser,
      url,
      outGif: join(outDir, "web-desktop.gif"),
      viewport: { width: 1280, height: 720 },
      script: desktopScript,
      width: 1200,
    });

    await record({
      browser,
      url,
      outGif: join(outDir, "web-mobile.gif"),
      device: devices["iPhone 13"],
      script: mobileScript,
      width: 600,
    });

    await browser.close();
  } finally {
    try {
      server.kill("SIGKILL");
    } catch {}
    try {
      spawnSync("tmux", ["kill-server"], { env });
    } catch {}
    try {
      rmSync(sandbox, { recursive: true, force: true });
    } catch {}
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
