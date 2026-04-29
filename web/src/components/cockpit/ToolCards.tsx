// Per-kind tool call renderers. Each component takes the started tool
// and (optionally) the completion row, and renders a card that fits the
// shape of the tool's inputs and outputs.
//
// Patterns inspired by Cursor agent chat and VSCode Copilot Chat: each
// tool feels purpose-built rather than a generic "tool ran" box.

import { useState } from "react";
import type { ActivityRow, ToolCall } from "../../lib/cockpitTypes";

interface Props {
  tool: ToolCall;
  result?: ActivityRow;
}

/**
 * Pick the right per-kind component. Falls back to the generic card.
 */
export function ToolCard({ tool, result }: Props) {
  switch (tool.kind) {
    case "execute":
      return <ExecuteToolCard tool={tool} result={result} />;
    case "read":
      return <ReadToolCard tool={tool} result={result} />;
    case "edit":
      return <EditToolCard tool={tool} result={result} />;
    case "delete":
      return <EditToolCard tool={tool} result={result} />;
    case "search":
      return <SearchToolCard tool={tool} result={result} />;
    case "fetch":
      return <FetchToolCard tool={tool} result={result} />;
    case "think":
      return <ThinkToolCard tool={tool} result={result} />;
    default:
      return <GenericToolCard tool={tool} result={result} />;
  }
}

/* ── shared bits ─────────────────────────────────────────────────── */

function statusFor(result?: ActivityRow): "running" | "ok" | "err" {
  if (!result) return "running";
  return result.kind === "tool_error" ? "err" : "ok";
}

function StatusDot({ status }: { status: "running" | "ok" | "err" }) {
  const cls =
    status === "running"
      ? "bg-brand-400 animate-pulse"
      : status === "ok"
        ? "bg-status-running"
        : "bg-status-error";
  return <span className={`h-2 w-2 shrink-0 rounded-full ${cls}`} />;
}

function StatusLabel({ status }: { status: "running" | "ok" | "err" }) {
  if (status === "running") return <span className="text-text-dim">running…</span>;
  if (status === "err") return <span className="text-status-error">failed</span>;
  return <span className="text-text-dim">done</span>;
}

function tryParseJson(s: string): Record<string, unknown> | null {
  try {
    const v = JSON.parse(s);
    return v && typeof v === "object" && !Array.isArray(v)
      ? (v as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function pickStr(o: Record<string, unknown> | null, ...keys: string[]): string | null {
  if (!o) return null;
  for (const k of keys) {
    const v = o[k];
    if (typeof v === "string") return v;
  }
  return null;
}

/* ── execute (bash) ─────────────────────────────────────────────── */

function ExecuteToolCard({ tool, result }: Props) {
  const status = statusFor(result);
  const args = tryParseJson(tool.args_preview);
  const command =
    pickStr(args, "command", "cmd", "args") ?? tool.args_preview;
  const description = pickStr(args, "description");
  const [open, setOpen] = useState(false);

  return (
    <div className="my-1 overflow-hidden rounded-md border border-surface-700 bg-surface-800/50 text-sm">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-surface-800"
      >
        <StatusDot status={status} />
        <span className="font-mono text-xs text-text-dim">$</span>
        <span className="truncate font-mono text-xs text-text-secondary">
          {command}
        </span>
        <span className="ml-auto text-[11px]">
          <StatusLabel status={status} />
        </span>
      </button>
      {description && !open && (
        <div className="border-t border-surface-800 px-3 py-1 text-[11px] text-text-dim italic">
          {description}
        </div>
      )}
      {open && (
        <pre className="border-t border-surface-800 bg-surface-950 px-3 py-2 text-xs font-mono text-text-secondary whitespace-pre-wrap break-all">
          {command}
        </pre>
      )}
    </div>
  );
}

/* ── read ───────────────────────────────────────────────────────── */

function ReadToolCard({ tool, result }: Props) {
  const status = statusFor(result);
  const args = tryParseJson(tool.args_preview);
  const path = pickStr(args, "path", "file_path", "filePath", "filename");
  const range = formatRange(args);

  return (
    <div className="my-1 flex items-center gap-2 rounded-md border border-surface-800 bg-surface-800/30 px-3 py-1.5 text-sm">
      <StatusDot status={status} />
      <span className="text-[11px] uppercase tracking-wider text-text-dim">read</span>
      <span className="truncate font-mono text-xs text-text-secondary">
        {path ?? tool.name}
      </span>
      {range && <span className="text-[11px] text-text-dim">{range}</span>}
      <span className="ml-auto text-[11px]">
        <StatusLabel status={status} />
      </span>
    </div>
  );
}

function formatRange(args: Record<string, unknown> | null): string | null {
  if (!args) return null;
  const offset = typeof args.offset === "number" ? args.offset : null;
  const limit = typeof args.limit === "number" ? args.limit : null;
  if (offset !== null && limit !== null) return `L${offset}–${offset + limit}`;
  if (offset !== null) return `from L${offset}`;
  if (limit !== null) return `${limit} lines`;
  return null;
}

/* ── edit / write / delete ──────────────────────────────────────── */

function EditToolCard({ tool, result }: Props) {
  const status = statusFor(result);
  const args = tryParseJson(tool.args_preview);
  const path = pickStr(args, "path", "file_path", "filePath", "filename");
  const oldText = pickStr(args, "old_string", "oldString", "old_str");
  const newText = pickStr(args, "new_string", "newString", "new_str", "content");
  const [open, setOpen] = useState(false);
  const hasDiff = (oldText !== null && oldText !== "") || (newText !== null && newText !== "");

  const verb = tool.kind === "delete" ? "delete" : oldText ? "edit" : "write";

  return (
    <div className="my-1 overflow-hidden rounded-md border border-surface-700 bg-surface-800/50 text-sm">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        disabled={!hasDiff}
        className={`flex w-full items-center gap-2 px-3 py-1.5 text-left ${hasDiff ? "hover:bg-surface-800 cursor-pointer" : "cursor-default"}`}
      >
        <StatusDot status={status} />
        <span className="text-[11px] uppercase tracking-wider text-text-dim">{verb}</span>
        <span className="truncate font-mono text-xs text-text-secondary">
          {path ?? tool.name}
        </span>
        <span className="ml-auto text-[11px]">
          <StatusLabel status={status} />
        </span>
      </button>
      {open && hasDiff && (
        <div className="border-t border-surface-800 bg-surface-950 font-mono text-[11px]">
          {oldText && (
            <pre className="overflow-x-auto whitespace-pre-wrap break-all bg-status-error/10 px-3 py-1 text-status-error/80">
              {oldText
                .split("\n")
                .map((l) => `- ${l}`)
                .join("\n")}
            </pre>
          )}
          {newText && (
            <pre className="overflow-x-auto whitespace-pre-wrap break-all bg-status-running/10 px-3 py-1 text-status-running/90">
              {newText
                .split("\n")
                .map((l) => `+ ${l}`)
                .join("\n")}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}

/* ── search ─────────────────────────────────────────────────────── */

function SearchToolCard({ tool, result }: Props) {
  const status = statusFor(result);
  const args = tryParseJson(tool.args_preview);
  const query =
    pickStr(args, "query", "pattern", "q", "search") ?? tool.args_preview;
  const path = pickStr(args, "path", "directory", "scope");

  return (
    <div className="my-1 flex items-center gap-2 rounded-md border border-surface-800 bg-surface-800/30 px-3 py-1.5 text-sm">
      <StatusDot status={status} />
      <span className="text-[11px] uppercase tracking-wider text-text-dim">search</span>
      <span className="truncate font-mono text-xs text-text-secondary">
        {query}
      </span>
      {path && (
        <span className="truncate text-[11px] text-text-dim">in {path}</span>
      )}
      <span className="ml-auto text-[11px]">
        <StatusLabel status={status} />
      </span>
    </div>
  );
}

/* ── fetch ──────────────────────────────────────────────────────── */

function FetchToolCard({ tool, result }: Props) {
  const status = statusFor(result);
  const args = tryParseJson(tool.args_preview);
  const url = pickStr(args, "url", "uri", "endpoint") ?? tool.name;

  return (
    <div className="my-1 flex items-center gap-2 rounded-md border border-surface-800 bg-surface-800/30 px-3 py-1.5 text-sm">
      <StatusDot status={status} />
      <span className="text-[11px] uppercase tracking-wider text-text-dim">fetch</span>
      <span className="truncate font-mono text-xs text-text-secondary">{url}</span>
      <span className="ml-auto text-[11px]">
        <StatusLabel status={status} />
      </span>
    </div>
  );
}

/* ── think ──────────────────────────────────────────────────────── */

function ThinkToolCard({ tool }: Props) {
  return (
    <div className="my-1 flex items-center gap-2 px-3 py-1 text-xs italic text-text-muted">
      <span className="h-1.5 w-1.5 rounded-full bg-text-dim" />
      <span>{tool.name || "thinking…"}</span>
    </div>
  );
}

/* ── generic fallback ───────────────────────────────────────────── */

function GenericToolCard({ tool, result }: Props) {
  const status = statusFor(result);
  const [open, setOpen] = useState(false);
  return (
    <div className="my-1 overflow-hidden rounded-md border border-surface-700 bg-surface-800/50 text-sm">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-surface-800"
      >
        <StatusDot status={status} />
        <span className="text-[11px] uppercase tracking-wider text-text-dim">{tool.kind || "tool"}</span>
        <span className="truncate font-mono text-xs text-text-secondary">
          {tool.name}
        </span>
        <span className="ml-auto text-[11px]">
          <StatusLabel status={status} />
        </span>
      </button>
      {open && tool.args_preview && (
        <pre className="border-t border-surface-800 bg-surface-950 px-3 py-2 text-xs font-mono text-text-dim whitespace-pre-wrap break-all">
          {tool.args_preview}
        </pre>
      )}
    </div>
  );
}
