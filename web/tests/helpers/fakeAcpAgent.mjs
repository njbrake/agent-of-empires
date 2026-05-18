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
//                          then return a stop response
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

async function emitSessionUpdates(sessionId, updates) {
  for (const u of updates) {
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
  authMethods: [],
};

async function handleRequest(msg) {
  const { id, method, params } = msg;
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
    } else if (msg.method) {
      // Notification from client (e.g. fs/* response). We don't model
      // delegated FS/terminal call results; tests that need them script
      // their turns to avoid triggering tool calls.
      process.stderr.write(
        `[fakeAcpAgent] received notification: ${msg.method}\n`,
      );
    }
    // Responses to our outbound requests (we don't make any) are ignored.
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
