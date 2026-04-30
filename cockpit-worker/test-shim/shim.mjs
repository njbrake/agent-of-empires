#!/usr/bin/env node
/**
 * Minimal ACP agent shim for cockpit integration tests. Does NOT call any
 * model. Replays a scripted sequence of session updates so we can verify
 * the Rust ACP client end-to-end without API keys or network access.
 *
 * Behavior on `prompt`:
 *   1. Emit one `agent_message_chunk` text event echoing the prompt.
 *   2. Emit a `tool_call` event with kind=read, status=pending.
 *   3. Emit a matching `tool_call_update` with status=completed.
 *   4. Emit a final `agent_message_chunk` saying "done".
 *   5. Resolve with stopReason=end_turn.
 *
 * Used by `tests/cockpit_acp_smoke.rs`.
 */

import * as acp from "@agentclientprotocol/sdk";
import { Readable, Writable } from "node:stream";

class ShimAgent {
  constructor(connection) {
    this.connection = connection;
    this.sessions = new Map();
  }

  async initialize(params) {
    return {
      protocolVersion: params.protocolVersion ?? acp.PROTOCOL_VERSION,
      agentCapabilities: {
        loadSession: false,
      },
    };
  }

  async authenticate(_params) {
    return {};
  }

  async newSession(_params) {
    const sessionId = "shim-" + Math.random().toString(36).slice(2, 10);
    this.sessions.set(sessionId, {});
    return { sessionId };
  }

  async setSessionMode(_params) {
    return {};
  }

  async prompt(params) {
    if (!this.sessions.has(params.sessionId)) {
      throw new Error("unknown session");
    }
    const userText = params.prompt
      .filter((c) => c.type === "text")
      .map((c) => c.text)
      .join("\n");

    // Optional slow path: tests that need to observe mid-turn UI
    // (e.g. the working spinner) include "SLOW" in the prompt so the
    // shim adds a configurable delay between events.
    const slow = userText.includes("SLOW");
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

    await this.connection.sessionUpdate({
      sessionId: params.sessionId,
      update: {
        sessionUpdate: "agent_message_chunk",
        content: { type: "text", text: `received: ${userText}` },
      },
    });
    if (slow) await sleep(800);

    await this.connection.sessionUpdate({
      sessionId: params.sessionId,
      update: {
        sessionUpdate: "tool_call",
        toolCallId: "tc-1",
        title: "Reading shim file",
        kind: "read",
        status: "pending",
        locations: [{ path: "/tmp/shim.txt" }],
        rawInput: { path: "/tmp/shim.txt" },
      },
    });
    if (slow) await sleep(800);

    await this.connection.sessionUpdate({
      sessionId: params.sessionId,
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "tc-1",
        status: "completed",
        rawOutput: { content: "shim file contents" },
      },
    });
    if (slow) await sleep(800);

    // Optional fs round-trip exercised by tests via prompt keywords.
    if (userText.includes("FS_READ_WRITE")) {
      try {
        // Write a fresh file inside the session cwd.
        await this.connection.writeTextFile({
          sessionId: params.sessionId,
          path: process.cwd() + "/shim-roundtrip.txt",
          content: "hello from shim",
        });
        // Read it back.
        const read = await this.connection.readTextFile({
          sessionId: params.sessionId,
          path: process.cwd() + "/shim-roundtrip.txt",
        });
        await this.connection.sessionUpdate({
          sessionId: params.sessionId,
          update: {
            sessionUpdate: "agent_message_chunk",
            content: { type: "text", text: `fs_read=${read.content}` },
          },
        });
      } catch (err) {
        await this.connection.sessionUpdate({
          sessionId: params.sessionId,
          update: {
            sessionUpdate: "agent_message_chunk",
            content: { type: "text", text: `fs_error=${err.message ?? err}` },
          },
        });
      }
    }

    // Optional terminal round-trip exercised by tests.
    if (userText.includes("TERMINAL_RUN")) {
      try {
        const term = await this.connection.createTerminal({
          sessionId: params.sessionId,
          command: "echo",
          args: ["terminal-roundtrip-ok"],
        });
        const exit = await term.waitForExit();
        const out = await term.currentOutput();
        // WaitForTerminalExitResponse flattens TerminalExitStatus, so
        // exitCode is at the top level. Fall back to nested in case the
        // SDK wraps it differently in a future version.
        const code =
          exit.exitCode ??
          exit.exit_code ??
          exit.exitStatus?.exitCode ??
          "?";
        await this.connection.sessionUpdate({
          sessionId: params.sessionId,
          update: {
            sessionUpdate: "agent_message_chunk",
            content: {
              type: "text",
              text: `terminal_output=${out.output.trim()};exit=${code}`,
            },
          },
        });
        await term.release();
      } catch (err) {
        await this.connection.sessionUpdate({
          sessionId: params.sessionId,
          update: {
            sessionUpdate: "agent_message_chunk",
            content: { type: "text", text: `terminal_error=${err.message ?? err}` },
          },
        });
      }
    }

    // Optional permission request, controlled by prompt content so tests
    // can opt into exercising the approval round-trip.
    if (userText.includes("REQUEST_PERMISSION")) {
      const response = await this.connection.requestPermission({
        sessionId: params.sessionId,
        toolCall: {
          toolCallId: "tc-2",
          title: "Modify shim config",
          kind: "edit",
          status: "pending",
          locations: [{ path: "/tmp/shim-config.json" }],
          rawInput: {
            path: "/tmp/shim-config.json",
            content: '{"x":1}',
          },
        },
        options: [
          { kind: "allow_once", name: "Allow once", optionId: "yes" },
          { kind: "reject_once", name: "Reject", optionId: "no" },
        ],
      });
      const verdict =
        response.outcome.outcome === "selected"
          ? response.outcome.optionId
          : "cancelled";
      await this.connection.sessionUpdate({
        sessionId: params.sessionId,
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: `permission_outcome=${verdict}` },
        },
      });
    }

    await this.connection.sessionUpdate({
      sessionId: params.sessionId,
      update: {
        sessionUpdate: "agent_message_chunk",
        content: { type: "text", text: "done" },
      },
    });

    return { stopReason: "end_turn" };
  }

  async cancel(_params) {
    // Shim doesn't track cancellable work.
  }
}

// AOE_ACP_SOCKET: when set, connect to that unix socket as the
// transport instead of using stdio. Used by sandboxed cockpit sessions
// (Docker bind-mounts the socket into the container) and for
// integration tests that exercise the socket transport.
import net from "node:net";
import { Duplex } from "node:stream";

async function bootstrap() {
  let inputWeb;
  let outputWeb;
  if (process.env.AOE_ACP_SOCKET) {
    const sock = await new Promise((resolve, reject) => {
      const s = net.createConnection(process.env.AOE_ACP_SOCKET, () =>
        resolve(s),
      );
      s.on("error", reject);
    });
    // The unix socket is a single bidirectional stream. acp.ndJsonStream
    // expects (writable, readable) so we hand it the socket twice via
    // Duplex.toWeb on both halves.
    inputWeb = Duplex.toWeb(sock).writable;
    outputWeb = Duplex.toWeb(sock).readable;
    sock.on("end", () => process.exit(0));
  } else {
    inputWeb = Writable.toWeb(process.stdout);
    outputWeb = Readable.toWeb(process.stdin);
  }
  const stream = acp.ndJsonStream(inputWeb, outputWeb);
  new acp.AgentSideConnection((conn) => new ShimAgent(conn), stream);

  process.stdin.on("end", () => process.exit(0));
  process.on("SIGTERM", () => process.exit(0));
  process.on("SIGINT", () => process.exit(0));
}

bootstrap().catch((err) => {
  console.error("[shim] bootstrap failed:", err);
  process.exit(1);
});
