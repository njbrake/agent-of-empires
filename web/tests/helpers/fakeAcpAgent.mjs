#!/usr/bin/env node
// Fake ACP agent for cockpit Playwright tests.
//
// Speaks just enough of the Agent Client Protocol (newline-delimited
// JSON-RPC 2.0) for `src/cockpit/acp_client.rs` to drive a turn:
//
//   initialize          -> return protocolVersion + agentCapabilities
//   session/new         -> return a deterministic sessionId
//   session/load        -> same shape; lets cockpit's Resume mode work
//   session/prompt      -> emit scripted session/update notifications,
//                          then return a stop response. Script entries
//                          with `sessionUpdate: "permission_request"`
//                          are translated into a real outbound
//                          `session/request_permission` JSON-RPC
//                          REQUEST; the fake awaits the client's
//                          response before continuing the turn.
//   session/setMode     -> emit current_mode_changed
//   session/cancel      -> emit stopped { stopReason: "cancelled" }
//
// Script source:
//
//   The env var FAKE_ACP_SCRIPT points to a JSON file describing the
//   event sequence(s) to emit per `session/prompt` call. If unset, the
//   default is a single happy-path turn that emits one agent_message_chunk
//   then stops.
//
// Script shape (rough):
//
//   {
//     "turns": [
//       {
//         "updates": [
//           { "sessionUpdate": "agent_message_chunk", "content": {...} },
//           ...
//         ],
//         "stopReason": "end_turn"
//       },
//       ...
//     ]
//   }
//
// Each `session/prompt` consumes one entry from `turns`. If the array is
// exhausted, subsequent prompts get the default happy-path turn.

import { createInterface } from "node:readline";
import { readFileSync, existsSync } from "node:fs";

const DEFAULT_TURN = {
  updates: [
    {
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "Hello from fake ACP agent." },
    },
  ],
  stopReason: "end_turn",
};

function loadScript() {
  const path = process.env.FAKE_ACP_SCRIPT;
  if (!path || !existsSync(path)) return { turns: [] };
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (err) {
    process.stderr.write(
      `[fakeAcpAgent] failed to parse FAKE_ACP_SCRIPT=${path}: ${err}\n`,
    );
    return { turns: [] };
  }
}

const script = loadScript();
let turnCursor = 0;

function nextTurn() {
  if (turnCursor < script.turns.length) {
    return script.turns[turnCursor++];
  }
  return DEFAULT_TURN;
}

function send(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

function sendResult(id, result) {
  send({ jsonrpc: "2.0", id, result });
}

function sendError(id, code, message) {
  send({ jsonrpc: "2.0", id, error: { code, message } });
}

function sendNotification(method, params) {
  send({ jsonrpc: "2.0", method, params });
}

// Track outbound requests we send to the client so we can resolve them
// when the client's response arrives. Keyed on the request id.
let nextOutboundId = 1;
const pendingOutbound = new Map();

// 15s ceiling so a wedged supervisor surfaces as an explicit timeout
// instead of a hung promise that stalls the script's remaining updates
// and leaves Playwright to time out generically.
const OUTBOUND_REQUEST_TIMEOUT_MS = 15_000;

function sendRequest(method, params) {
  const id = `fake-acp-req-${nextOutboundId++}`;
  send({ jsonrpc: "2.0", id, method, params });
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      if (pendingOutbound.delete(id)) {
        reject(
          new Error(
            `fakeAcpAgent: outbound ${method} id=${id} timed out after ${OUTBOUND_REQUEST_TIMEOUT_MS}ms`,
          ),
        );
      }
    }, OUTBOUND_REQUEST_TIMEOUT_MS);
    pendingOutbound.set(id, {
      resolve: (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      reject: (err) => {
        clearTimeout(timer);
        reject(err);
      },
    });
  });
}

function resolveOutbound(msg) {
  const entry = pendingOutbound.get(msg.id);
  if (!entry) {
    process.stderr.write(
      `[fakeAcpAgent] response for unknown id ${msg.id}\n`,
    );
    return;
  }
  pendingOutbound.delete(msg.id);
  if (msg.error) {
    entry.reject(msg.error);
  } else {
    entry.resolve(msg.result);
  }
}

// Default permission options when a `permission_request` script entry
// omits its own. Covers every PermissionOptionKind aoe's
// `pick_option_id` (src/cockpit/acp_client.rs) consults so an
// `allow` / `allow_always` / `deny` decision always maps cleanly.
const DEFAULT_PERMISSION_OPTIONS = [
  { optionId: "allow-once", name: "Allow once", kind: "allow_once" },
  { optionId: "allow-always", name: "Allow always", kind: "allow_always" },
  { optionId: "reject-once", name: "Reject once", kind: "reject_once" },
  { optionId: "reject-always", name: "Reject always", kind: "reject_always" },
];

async function emitSessionUpdates(sessionId, updates) {
  for (const u of updates) {
    if (u && u.sessionUpdate === "permission_request") {
      // ACP carries permissions on a separate JSON-RPC request, not on
      // session/update. Translate the scripted entry into a real
      // `session/request_permission` request and wait for the client's
      // decision before continuing the turn so the assertions in the
      // approval spec can observe ApprovalRequested + resolve it.
      await sendRequest("session/request_permission", {
        sessionId,
        toolCall: u.toolCall ?? {
          toolCallId: `fake-tool-call-${Date.now()}`,
          title: "fake tool call",
          kind: "edit",
        },
        options: u.options ?? DEFAULT_PERMISSION_OPTIONS,
      }).catch((err) => {
        process.stderr.write(
          `[fakeAcpAgent] permission_request rejected: ${JSON.stringify(err)}\n`,
        );
      });
      continue;
    }
    sendNotification("session/update", { sessionId, update: u });
    // Tiny tick between updates so the cockpit reducer can apply each
    // event in order rather than batching them.
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
}

function makeSessionId() {
  return `fake-acp-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

const INITIALIZE_RESULT = {
  protocolVersion: 1,
  agentCapabilities: {
    loadSession: true,
    promptCapabilities: {
      image: false,
      embeddedContext: false,
    },
    mcpCapabilities: {
      http: false,
      sse: false,
    },
  },
  agentInfo: {
    name: "@agentclientprotocol/claude-agent-acp",
    version: "0.37.0",
  },
  // No authMethods key at all. An empty array is interpreted by some
  // ACP client implementations as "auth methods listed but none
  // available", which surfaces as AuthRequired on the next call.
  // Omitting the key signals "no auth required" cleanly.
};

async function handleRequest(msg) {
  const { id, method, params } = msg;
  if (process.env.FAKE_ACP_DEBUG) {
    try {
      const { appendFileSync } = await import("node:fs");
      appendFileSync(
        process.env.FAKE_ACP_DEBUG,
        `req method=${method} id=${id} params=${JSON.stringify(params).slice(0, 200)}\n`,
      );
    } catch {
      // ignore log errors
    }
  }
  switch (method) {
    case "initialize":
      sendResult(id, INITIALIZE_RESULT);
      return;

    case "session/new":
    case "session/load": {
      const sessionId = params?.sessionId ?? makeSessionId();
      sendResult(id, { sessionId });
      return;
    }

    case "session/setMode": {
      const sessionId = params?.sessionId;
      const modeId = params?.modeId;
      sendResult(id, {});
      if (sessionId && modeId) {
        await emitSessionUpdates(sessionId, [
          { sessionUpdate: "current_mode_changed", currentModeId: modeId },
        ]);
      }
      return;
    }

    case "session/cancel": {
      const sessionId = params?.sessionId;
      sendResult(id, {});
      if (sessionId) {
        await emitSessionUpdates(sessionId, [
          { sessionUpdate: "stopped", stopReason: "cancelled" },
        ]);
      }
      return;
    }

    case "session/prompt": {
      const sessionId = params?.sessionId;
      const turn = nextTurn();
      if (sessionId) {
        await emitSessionUpdates(sessionId, turn.updates);
      }
      sendResult(id, { stopReason: turn.stopReason ?? "end_turn" });
      return;
    }

    default:
      sendError(id, -32601, `fakeAcpAgent: method '${method}' not implemented`);
  }
}

async function main() {
  const rl = createInterface({ input: process.stdin });
  rl.on("line", async (line) => {
    const trimmed = line.trim();
    if (!trimmed) return;
    let msg;
    try {
      msg = JSON.parse(trimmed);
    } catch (err) {
      process.stderr.write(`[fakeAcpAgent] bad JSON: ${err}\n`);
      return;
    }
    if (msg.id !== undefined && msg.method) {
      try {
        await handleRequest(msg);
      } catch (err) {
        process.stderr.write(`[fakeAcpAgent] handler error: ${err}\n`);
        sendError(msg.id, -32603, `internal: ${err}`);
      }
    } else if (msg.id !== undefined && (msg.result !== undefined || msg.error !== undefined)) {
      // Response to one of our outbound requests (e.g.
      // session/request_permission). Resolve the awaiting Promise.
      resolveOutbound(msg);
    } else if (msg.method) {
      // Notification from client (e.g. fs/* response). We don't model
      // delegated FS/terminal call results; tests that need them script
      // their turns to avoid triggering tool calls.
      process.stderr.write(
        `[fakeAcpAgent] received notification: ${msg.method}\n`,
      );
    }
  });

  rl.on("close", () => {
    process.exit(0);
  });

  process.on("SIGTERM", () => process.exit(0));
  process.on("SIGINT", () => process.exit(0));
}

main().catch((err) => {
  process.stderr.write(`[fakeAcpAgent] fatal: ${err}\n`);
  process.exit(1);
});
