// Per-kind tool call renderers. Each component takes the started tool
// and (optionally) the completion row, and renders a card that fits
// the shape of the tool's inputs and outputs.
//
// Patterns inspired by Cursor agent chat and VSCode Copilot Chat: each
// tool feels purpose-built rather than a generic "tool ran" box. We
// surface the key fields (path, command, query) inline in the card
// header and put output in a syntax-highlighted body.

import { useEffect, useMemo, useState, type CSSProperties } from "react";
import ReactDiffViewer, { DiffMethod } from "react-diff-viewer-continued";
import {
  ChevronDown,
  Copy as CopyIcon,
  FileText,
  Globe,
  Pencil,
  Search,
  Sparkles,
  Terminal,
  Trash2,
} from "lucide-react";

import { getHighlighter, langKeyForExt, loadLanguage } from "../../lib/highlighter";
import { hasAnsi, parseAnsi, type AnsiStyle } from "../../lib/ansi";
import { parseJsonObject, pickFirst, pickStr } from "../../lib/cockpitArgs";
import type { ActivityRow, ToolCall } from "../../lib/cockpitTypes";

interface Props {
  tool: ToolCall;
  result?: ActivityRow;
}

export function ToolCard({ tool, result }: Props) {
  switch (tool.kind) {
    case "execute":
      return <ExecuteToolCard tool={tool} result={result} />;
    case "read":
      return <ReadToolCard tool={tool} result={result} />;
    case "edit":
      return <EditToolCard tool={tool} result={result} />;
    case "delete":
      return <DeleteToolCard tool={tool} result={result} />;
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

/* ── Shared header bits ──────────────────────────────────────────── */

type Status = "running" | "ok" | "err";

function statusFor(result?: ActivityRow): Status {
  if (!result) return "running";
  return result.kind === "tool_error" ? "err" : "ok";
}

function StatusDot({ status }: { status: Status }) {
  const cls =
    status === "running"
      ? "bg-brand-400 animate-pulse"
      : status === "ok"
        ? "bg-status-running"
        : "bg-status-error";
  return <span className={`h-2 w-2 shrink-0 rounded-full ${cls}`} />;
}

function StatusBadge({ status }: { status: Status }) {
  if (status === "running") {
    return (
      <span className="inline-flex items-center gap-1 text-[11px] text-text-dim">
        <span className="h-1.5 w-1.5 rounded-full bg-brand-400 animate-pulse" />
        running
      </span>
    );
  }
  if (status === "err") {
    return <span className="text-[11px] text-status-error">failed</span>;
  }
  return <span className="text-[11px] text-text-dim">done</span>;
}

interface CardChromeProps {
  status: Status;
  icon: React.ReactNode;
  label: string;
  primary: React.ReactNode;
  meta?: React.ReactNode;
  expanded: boolean;
  onToggle?: () => void;
  body?: React.ReactNode;
}

function CardChrome({
  status,
  icon,
  label,
  primary,
  meta,
  expanded,
  onToggle,
  body,
}: CardChromeProps) {
  const Header = onToggle ? "button" : "div";
  return (
    <div className="my-1 overflow-hidden rounded-md border border-surface-700 bg-surface-800/50 text-sm">
      <Header
        type={onToggle ? "button" : undefined}
        onClick={onToggle}
        className={[
          "flex w-full items-center gap-2 px-3 py-1.5 text-left",
          onToggle ? "cursor-pointer hover:bg-surface-800" : "",
        ].join(" ")}
      >
        <StatusDot status={status} />
        <span className="text-text-dim">{icon}</span>
        <span className="text-[11px] uppercase tracking-wider text-text-dim">
          {label}
        </span>
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-text-secondary">
          {primary}
        </span>
        {meta}
        <StatusBadge status={status} />
        {onToggle && (
          <ChevronDown
            className={[
              "h-3.5 w-3.5 text-text-dim transition-transform",
              expanded ? "rotate-180" : "",
            ].join(" ")}
          />
        )}
      </Header>
      {expanded && body}
    </div>
  );
}

/* ── Helpers ─────────────────────────────────────────────────────── */

function truncateLines(text: string, max: number): {
  shown: string;
  truncated: number;
} {
  const lines = text.split("\n");
  if (lines.length <= max) return { shown: text, truncated: 0 };
  return {
    shown: lines.slice(0, max).join("\n"),
    truncated: lines.length - max,
  };
}

function copy(text: string) {
  navigator.clipboard?.writeText(text).catch(() => {});
}

function CopyButton({ text }: { text: string }) {
  return (
    <button
      type="button"
      title="Copy"
      onClick={(e) => {
        e.stopPropagation();
        copy(text);
      }}
      className="rounded p-1 text-text-dim hover:bg-surface-800 hover:text-text-secondary"
    >
      <CopyIcon className="h-3 w-3" />
    </button>
  );
}

/* ── Highlighted code block (used by Read, Edit, Execute output) ── */

/** If the input is a single outer markdown code fence (```lang ... ```),
 *  strip the fence and return the inner body plus the fence's language
 *  hint. Tool output emitted by ACP agents (Claude in particular) is
 *  routinely pre-wrapped in fenced blocks like ```console ...``` — left
 *  un-stripped, the cards render literal backticks above the content. */
function unwrapMarkdownFence(text: string): {
  text: string;
  lang: string | null;
} {
  const m = text.match(/^```([\w+-]+)?\s*\n([\s\S]*?)\n```\s*$/);
  if (!m) return { text, lang: null };
  return { text: m[2] ?? "", lang: m[1] ?? null };
}

function HighlightedBlock({
  text,
  language,
  maxLines = 20,
}: {
  text: string;
  language?: string;
  maxLines?: number;
}) {
  const [html, setHtml] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);
  const unwrapped = unwrapMarkdownFence(text);
  const effectiveText = unwrapped.text;
  const effectiveLang = unwrapped.lang ?? language;
  const { shown, truncated } = truncateLines(
    effectiveText,
    showAll ? 1_000_000 : maxLines,
  );

  // ANSI fast path: when the text carries SGR escape sequences (e.g.
  // `gls --color=always`, `git status --color=always`), Shiki's bash
  // grammar can't handle them — it would either render the literal
  // `[01;34m` noise or fail to highlight at all. Render the styled
  // segments directly instead.
  const ansi = hasAnsi(shown);

  useEffect(() => {
    if (ansi) return;
    let cancelled = false;
    if (!effectiveLang) return;
    (async () => {
      try {
        const langKey = langKeyForExt(effectiveLang) ?? effectiveLang;
        await loadLanguage(langKey);
        const hl = await getHighlighter();
        if (cancelled) return;
        const out = hl.codeToHtml(shown, {
          lang: langKey,
          theme: "github-dark",
        });
        setHtml(out);
      } catch {
        // unknown language — fall back to plain
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [effectiveLang, shown]);

  return (
    <div className="border-t border-surface-800 bg-surface-950">
      {ansi ? (
        <AnsiBlock text={shown} />
      ) : html ? (
        <div
          className="overflow-x-auto px-3 py-2 text-xs [&_pre]:!bg-transparent [&_pre]:!m-0 [&_pre]:!p-0"
          dangerouslySetInnerHTML={{ __html: html }}
        />
      ) : (
        <pre className="overflow-x-auto px-3 py-2 text-xs font-mono text-text-secondary whitespace-pre-wrap break-all">
          {shown}
        </pre>
      )}
      {truncated > 0 && (
        <button
          type="button"
          onClick={() => setShowAll(true)}
          className="block w-full border-t border-surface-800 px-3 py-1 text-center text-[11px] text-text-dim hover:bg-surface-800"
        >
          Show {truncated} more line{truncated === 1 ? "" : "s"}
        </button>
      )}
    </div>
  );
}

/** Render text with embedded ANSI SGR codes as styled spans. We use
 *  `whitespace-pre` (not `pre-wrap`) because terminal output is
 *  column-sensitive; wrapping mangles tabular layouts like `ps aux`
 *  or `df -h`. */
function AnsiBlock({ text }: { text: string }) {
  const segments = useMemo(() => parseAnsi(text), [text]);
  return (
    <pre className="overflow-x-auto px-3 py-2 text-xs font-mono text-text-primary whitespace-pre">
      {segments.map((seg, i) => (
        <span key={i} style={ansiSegmentStyle(seg.style)}>
          {seg.text}
        </span>
      ))}
    </pre>
  );
}

function ansiSegmentStyle(style: AnsiStyle): CSSProperties {
  // Inverse swaps fg/bg before applying.
  const fg = style.inverse ? style.bg : style.fg;
  const bg = style.inverse ? style.fg : style.bg;
  return {
    color: fg,
    backgroundColor: bg,
    fontWeight: style.bold ? 600 : undefined,
    fontStyle: style.italic ? "italic" : undefined,
    textDecoration: style.underline ? "underline" : undefined,
    opacity: style.dim ? 0.65 : undefined,
  };
}

/* ── execute (bash) ─────────────────────────────────────────────── */

function ExecuteToolCard({ tool, result }: Props) {
  const status = statusFor(result);
  const args = parseJsonObject(tool.args_preview);
  const argCommand = pickStr(args, "command", "cmd", "args");
  // Fallback chain: real command → ACP-provided title (forwarded via
  // _aoe_title in CockpitRuntime) → tool's own kind/name. Never show
  // the literal `{}` from an empty raw_input.
  const title = pickStr(args, "_aoe_title");
  const command = pickFirst(argCommand, title, tool.name) ?? "(no command)";
  const description = pickStr(args, "description");
  const output = result?.text ?? "";
  const [open, setOpen] = useState(false);

  const meta =
    output && status !== "running" ? (
      <span className="hidden md:inline text-[11px] text-text-dim">
        {unwrapMarkdownFence(output).text.split("\n").length} lines
      </span>
    ) : undefined;

  return (
    <CardChrome
      status={status}
      icon={<Terminal className="h-3.5 w-3.5" />}
      label="bash"
      primary={
        <>
          <span className="mr-1 text-text-dim">$</span>
          {command}
        </>
      }
      meta={meta}
      expanded={open}
      onToggle={() => setOpen((v) => !v)}
      body={
        <>
          {description && (
            <div className="border-t border-surface-800 bg-surface-900/40 px-3 py-1 text-[11px] text-text-muted italic">
              {description}
            </div>
          )}
          {/* Full command — the chrome's primary slot is single-line
              truncated, so we surface the untruncated command here so
              users can read and copy it. Shiki's bash grammar gives
              the same coloring as our markdown code blocks. */}
          <HighlightedBlock text={command} language="bash" maxLines={6} />
          {output ? (
            <HighlightedBlock text={output} language="bash" maxLines={20} />
          ) : (
            <div className="border-t border-surface-800 bg-surface-950 px-3 py-2 text-[11px] text-text-dim italic">
              {status === "running" ? "Running…" : "(no output)"}
            </div>
          )}
        </>
      }
    />
  );
}

/* ── read ───────────────────────────────────────────────────────── */

function ReadToolCard({ tool, result }: Props) {
  const status = statusFor(result);
  const args = parseJsonObject(tool.args_preview);
  const argPath = pickStr(args, "path", "file_path", "filePath", "filename");
  const title = pickStr(args, "_aoe_title");
  const path = pickFirst(argPath, title, tool.name) ?? "(unknown file)";
  const range = formatRange(args);
  const ext = argPath?.match(/\.([a-z0-9]+)$/i)?.[1]?.toLowerCase();
  const content = result?.text ?? "";
  const [open, setOpen] = useState(false);

  const meta = content && (
    <span className="hidden md:inline text-[11px] text-text-dim">
      {content.split("\n").length} lines
    </span>
  );

  return (
    <CardChrome
      status={status}
      icon={<FileText className="h-3.5 w-3.5" />}
      label="read"
      primary={path}
      meta={
        <>
          {range && <span className="text-[11px] text-text-dim">{range}</span>}
          {meta}
        </>
      }
      expanded={open}
      onToggle={content ? () => setOpen((v) => !v) : undefined}
      body={
        content && (
          <HighlightedBlock text={content} language={ext} maxLines={16} />
        )
      }
    />
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

/* ── edit / write ───────────────────────────────────────────────── */

function EditToolCard({ tool, result }: Props) {
  const status = statusFor(result);
  const args = parseJsonObject(tool.args_preview);
  const argPath = pickStr(args, "path", "file_path", "filePath", "filename");
  const title = pickStr(args, "_aoe_title");
  const path = pickFirst(argPath, title, tool.name) ?? "(unknown file)";
  const oldText = pickStr(args, "old_string", "oldString", "old_str") ?? "";
  const newText =
    pickStr(args, "new_string", "newString", "new_str", "content") ?? "";
  const [open, setOpen] = useState(false);
  const hasDiff = oldText !== "" || newText !== "";
  const verb = oldText ? "edit" : "write";

  const adds = newText ? newText.split("\n").length : 0;
  const dels = oldText ? oldText.split("\n").length : 0;
  const meta = hasDiff && (
    <span className="hidden md:inline text-[11px]">
      <span className="text-emerald-400">+{adds}</span>{" "}
      <span className="text-rose-400">−{dels}</span>
    </span>
  );

  return (
    <CardChrome
      status={status}
      icon={<Pencil className="h-3.5 w-3.5" />}
      label={verb}
      primary={path}
      meta={meta}
      expanded={open}
      onToggle={hasDiff ? () => setOpen((v) => !v) : undefined}
      body={
        hasDiff && (
          <div className="cockpit-diff border-t border-surface-800">
            <ReactDiffViewer
              oldValue={oldText}
              newValue={newText}
              splitView={false}
              useDarkTheme
              compareMethod={DiffMethod.WORDS}
              hideLineNumbers={false}
              extraLinesSurroundingDiff={0}
              styles={DIFF_STYLES}
            />
          </div>
        )
      }
    />
  );
}

/** Theme overrides for react-diff-viewer-continued — drag its colors
 *  toward our zinc/brand palette so the diff doesn't look like it was
 *  pasted in from another app. */
const DIFF_STYLES = {
  variables: {
    dark: {
      diffViewerBackground: "var(--color-surface-950)",
      diffViewerColor: "var(--color-text-primary)",
      addedBackground: "rgba(34, 197, 94, 0.08)",
      addedColor: "rgb(187, 247, 208)",
      removedBackground: "rgba(239, 68, 68, 0.08)",
      removedColor: "rgb(254, 202, 202)",
      wordAddedBackground: "rgba(34, 197, 94, 0.20)",
      wordRemovedBackground: "rgba(239, 68, 68, 0.20)",
      addedGutterBackground: "rgba(34, 197, 94, 0.05)",
      removedGutterBackground: "rgba(239, 68, 68, 0.05)",
      gutterBackground: "var(--color-surface-900)",
      gutterBackgroundDark: "var(--color-surface-900)",
      highlightBackground: "var(--color-surface-800)",
      highlightGutterBackground: "var(--color-surface-800)",
      codeFoldGutterBackground: "var(--color-surface-900)",
      codeFoldBackground: "var(--color-surface-900)",
      emptyLineBackground: "var(--color-surface-950)",
      gutterColor: "var(--color-text-dim)",
      addedGutterColor: "rgb(187, 247, 208)",
      removedGutterColor: "rgb(254, 202, 202)",
      codeFoldContentColor: "var(--color-text-dim)",
      diffViewerTitleBackground: "var(--color-surface-900)",
      diffViewerTitleColor: "var(--color-text-secondary)",
      diffViewerTitleBorderColor: "var(--color-surface-800)",
    },
  },
  contentText: {
    fontSize: "11px",
    fontFamily:
      "'Geist Mono', ui-monospace, 'SFMono-Regular', monospace",
  },
  gutter: {
    fontSize: "10px",
    minWidth: "32px",
    padding: "0 6px",
  },
} as const;

/* ── delete ─────────────────────────────────────────────────────── */

function DeleteToolCard({ tool, result }: Props) {
  const status = statusFor(result);
  const args = parseJsonObject(tool.args_preview);
  const argPath = pickStr(args, "path", "file_path", "filePath", "filename");
  const title = pickStr(args, "_aoe_title");
  const path = pickFirst(argPath, title, tool.name) ?? "(unknown file)";
  return (
    <CardChrome
      status={status}
      icon={<Trash2 className="h-3.5 w-3.5 text-rose-400" />}
      label="delete"
      primary={path}
      expanded={false}
    />
  );
}

/* ── search ─────────────────────────────────────────────────────── */

function SearchToolCard({ tool, result }: Props) {
  const status = statusFor(result);
  const args = parseJsonObject(tool.args_preview);
  const argQuery = pickStr(args, "query", "pattern", "q", "search");
  const title = pickStr(args, "_aoe_title");
  const query = pickFirst(argQuery, title, tool.name) ?? "(no query)";
  const path = pickStr(args, "path", "directory", "scope");
  const output = result?.text ?? "";
  const lines = output ? output.split("\n").filter(Boolean) : [];
  const [open, setOpen] = useState(false);

  return (
    <CardChrome
      status={status}
      icon={<Search className="h-3.5 w-3.5" />}
      label="search"
      primary={query}
      meta={
        <>
          {path && (
            <span className="hidden md:inline text-[11px] text-text-dim">
              in {path}
            </span>
          )}
          {lines.length > 0 && (
            <span className="text-[11px] text-text-dim">
              {lines.length} match{lines.length === 1 ? "" : "es"}
            </span>
          )}
        </>
      }
      expanded={open}
      onToggle={lines.length > 0 ? () => setOpen((v) => !v) : undefined}
      body={
        lines.length > 0 && (
          <div className="border-t border-surface-800 bg-surface-950 max-h-64 overflow-y-auto">
            {lines.slice(0, 50).map((l, i) => (
              <div
                key={i}
                className="flex font-mono text-[11px] hover:bg-surface-900"
              >
                <span className="select-none w-10 shrink-0 px-2 py-0.5 text-right text-text-dim">
                  {i + 1}
                </span>
                <span className="px-2 py-0.5 text-text-secondary truncate">
                  {l}
                </span>
              </div>
            ))}
            {lines.length > 50 && (
              <div className="border-t border-surface-800 px-3 py-1 text-center text-[11px] text-text-dim">
                {lines.length - 50} more match{lines.length - 50 === 1 ? "" : "es"}
              </div>
            )}
          </div>
        )
      }
    />
  );
}

/* ── fetch ──────────────────────────────────────────────────────── */

function FetchToolCard({ tool, result }: Props) {
  const status = statusFor(result);
  const args = parseJsonObject(tool.args_preview);
  const argUrl = pickStr(args, "url", "uri", "endpoint");
  const title = pickStr(args, "_aoe_title");
  const url = pickFirst(argUrl, title, tool.name) ?? "(no url)";
  const output = result?.text ?? "";
  const [open, setOpen] = useState(false);

  return (
    <CardChrome
      status={status}
      icon={<Globe className="h-3.5 w-3.5" />}
      label="fetch"
      primary={url}
      expanded={open}
      onToggle={output ? () => setOpen((v) => !v) : undefined}
      body={
        output && <HighlightedBlock text={output} language="json" maxLines={16} />
      }
    />
  );
}

/* ── think ──────────────────────────────────────────────────────── */

function ThinkToolCard({ tool }: Props) {
  return (
    <div className="my-1 flex items-center gap-2 px-3 py-1 text-xs italic text-text-muted">
      <Sparkles className="h-3 w-3 text-text-dim" />
      <span>{tool.name || "thinking…"}</span>
    </div>
  );
}

/* ── generic fallback ───────────────────────────────────────────── */

function GenericToolCard({ tool, result }: Props) {
  const status = statusFor(result);
  const [open, setOpen] = useState(false);
  const output = result?.text ?? "";
  return (
    <CardChrome
      status={status}
      icon={<Sparkles className="h-3.5 w-3.5" />}
      label={tool.kind || "tool"}
      primary={tool.name}
      expanded={open}
      onToggle={tool.args_preview || output ? () => setOpen((v) => !v) : undefined}
      body={
        <>
          {tool.args_preview && (
            <div className="border-t border-surface-800 bg-surface-950 px-3 py-2">
              <div className="mb-1 flex items-center justify-between text-[10px] uppercase tracking-wider text-text-dim">
                <span>input</span>
                <CopyButton text={tool.args_preview} />
              </div>
              <pre className="overflow-x-auto font-mono text-[11px] text-text-muted whitespace-pre-wrap break-all">
                {tool.args_preview}
              </pre>
            </div>
          )}
          {output && (
            <div className="border-t border-surface-800 bg-surface-950 px-3 py-2">
              <div className="mb-1 flex items-center justify-between text-[10px] uppercase tracking-wider text-text-dim">
                <span>output</span>
                <CopyButton text={output} />
              </div>
              <pre className="overflow-x-auto font-mono text-[11px] text-text-secondary whitespace-pre-wrap break-all">
                {output}
              </pre>
            </div>
          )}
        </>
      }
    />
  );
}
