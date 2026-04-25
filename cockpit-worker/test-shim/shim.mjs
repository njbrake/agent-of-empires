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

    await this.connection.sessionUpdate({
      sessionId: params.sessionId,
      update: {
        sessionUpdate: "agent_message_chunk",
        content: { type: "text", text: `received: ${userText}` },
      },
    });

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

    await this.connection.sessionUpdate({
      sessionId: params.sessionId,
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "tc-1",
        status: "completed",
        rawOutput: { content: "shim file contents" },
      },
    });

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

const input = Writable.toWeb(process.stdout);
const output = Readable.toWeb(process.stdin);
const stream = acp.ndJsonStream(input, output);
new acp.AgentSideConnection((conn) => new ShimAgent(conn), stream);

process.stdin.on("end", () => process.exit(0));
process.on("SIGTERM", () => process.exit(0));
process.on("SIGINT", () => process.exit(0));
