#!/usr/bin/env node
/**
 * aoe-agent: ACP server wrapping Vercel AI SDK 6.
 *
 * One Node process per cockpit session. Accepts ACP requests from aoe
 * (the Rust ACP client) on stdin/stdout, drives a Vercel AI SDK loop
 * against the user's chosen provider, and streams structured events
 * back as ACP `session/update` notifications.
 *
 * MVP scope (v0): text-only Anthropic. Tools (file IO + bash) delegate
 * back to aoe via ACP `fs/*` and `terminal/*` in the next slice.
 *
 * Lifecycle: stdin closes -> exit 0. SIGTERM -> graceful shutdown.
 */

import * as acp from "@agentclientprotocol/sdk";
import { Readable, Writable } from "node:stream";
import { streamText } from "ai";
import { anthropic } from "@ai-sdk/anthropic";
import { openai } from "@ai-sdk/openai";
import { google } from "@ai-sdk/google";

interface SessionState {
  pendingPrompt: AbortController | null;
  modelId: string;
  /** Conversation history accumulated across turns within this session. */
  messages: Array<{ role: "user" | "assistant"; content: string }>;
}

class AoeAgent implements acp.Agent {
  private connection: acp.AgentSideConnection;
  private sessions: Map<string, SessionState>;

  constructor(connection: acp.AgentSideConnection) {
    this.connection = connection;
    this.sessions = new Map();
  }

  async initialize(
    params: acp.InitializeRequest,
  ): Promise<acp.InitializeResponse> {
    return {
      protocolVersion: params.protocolVersion ?? acp.PROTOCOL_VERSION,
      agentCapabilities: {
        loadSession: false,
        promptCapabilities: {
          // We accept text content blocks. Image/audio later.
          image: false,
          audio: false,
        },
      },
    };
  }

  async authenticate(
    _params: acp.AuthenticateRequest,
  ): Promise<acp.AuthenticateResponse | void> {
    return {};
  }

  async newSession(
    _params: acp.NewSessionRequest,
  ): Promise<acp.NewSessionResponse> {
    const sessionId = randomHexId();
    // Provider/model pick: env wins; default to Anthropic.
    const modelId = process.env.AOE_AGENT_MODEL ?? "claude-opus-4-7";
    this.sessions.set(sessionId, {
      pendingPrompt: null,
      modelId,
      messages: [],
    });
    return { sessionId };
  }

  async setSessionMode(
    _params: acp.SetSessionModeRequest,
  ): Promise<acp.SetSessionModeResponse> {
    // MVP: mode-switching is acknowledged but not enforced. Plan-mode
    // wiring lands when Vercel AI SDK gates tool execution accordingly.
    return {};
  }

  async prompt(params: acp.PromptRequest): Promise<acp.PromptResponse> {
    const session = this.sessions.get(params.sessionId);
    if (!session) {
      throw new Error(`Session ${params.sessionId} not found`);
    }

    // Cancel any in-flight turn.
    session.pendingPrompt?.abort();
    session.pendingPrompt = new AbortController();
    const abortSignal = session.pendingPrompt.signal;

    // Extract user prompt text.
    const userText = params.prompt
      .filter((c): c is acp.TextContentBlock => c.type === "text")
      .map((c) => c.text)
      .join("\n");

    session.messages.push({ role: "user", content: userText });

    try {
      const model = pickModel(session.modelId);
      const result = streamText({
        model,
        messages: session.messages,
        abortSignal,
      });

      let assistantBuffer = "";
      for await (const part of result.fullStream) {
        if (abortSignal.aborted) {
          break;
        }
        if (part.type === "text-delta") {
          // ai sdk 6 uses `text` field on text-delta parts.
          // Older versions used `textDelta`. Support both at runtime.
          const delta =
            (part as { text?: string }).text ??
            (part as { textDelta?: string }).textDelta ??
            "";
          if (!delta) continue;
          assistantBuffer += delta;
          await this.connection.sessionUpdate({
            sessionId: params.sessionId,
            update: {
              sessionUpdate: "agent_message_chunk",
              content: { type: "text", text: delta },
            },
          });
        } else if (part.type === "error") {
          const err = (part as { error: unknown }).error;
          throw err instanceof Error ? err : new Error(String(err));
        }
      }

      session.messages.push({ role: "assistant", content: assistantBuffer });

      if (abortSignal.aborted) {
        return { stopReason: "cancelled" };
      }
      session.pendingPrompt = null;
      return { stopReason: "end_turn" };
    } catch (err) {
      session.pendingPrompt = null;
      if (abortSignal.aborted) {
        return { stopReason: "cancelled" };
      }
      // Surface the error as an agent message chunk so the cockpit can
      // render it instead of dropping the whole turn.
      const message = err instanceof Error ? err.message : String(err);
      await this.connection
        .sessionUpdate({
          sessionId: params.sessionId,
          update: {
            sessionUpdate: "agent_message_chunk",
            content: {
              type: "text",
              text: `\n[aoe-agent error] ${message}\n`,
            },
          },
        })
        .catch(() => undefined);
      throw err;
    }
  }

  async cancel(params: acp.CancelNotification): Promise<void> {
    this.sessions.get(params.sessionId)?.pendingPrompt?.abort();
  }
}

function pickModel(modelId: string) {
  // Cheap dispatcher. Real impl will read aoe settings to pick provider
  // explicitly; for now we sniff the id.
  if (modelId.startsWith("claude-") || modelId.startsWith("anthropic:")) {
    return anthropic(modelId.replace(/^anthropic:/, ""));
  }
  if (modelId.startsWith("gpt-") || modelId.startsWith("openai:")) {
    return openai(modelId.replace(/^openai:/, ""));
  }
  if (modelId.startsWith("gemini-") || modelId.startsWith("google:")) {
    return google(modelId.replace(/^google:/, ""));
  }
  // Fallback to anthropic.
  return anthropic(modelId);
}

function randomHexId(): string {
  return Array.from(crypto.getRandomValues(new Uint8Array(16)))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

// Bootstrap: wire stdin/stdout, build the connection.
function main() {
  const input = Writable.toWeb(process.stdout);
  const output = Readable.toWeb(process.stdin) as ReadableStream<Uint8Array>;
  const stream = acp.ndJsonStream(input, output);
  new acp.AgentSideConnection((conn) => new AoeAgent(conn), stream);

  // Exit cleanly on stdin close.
  process.stdin.on("end", () => process.exit(0));
  process.on("SIGTERM", () => process.exit(0));
  process.on("SIGINT", () => process.exit(0));
}

main();
