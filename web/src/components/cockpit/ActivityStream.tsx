// Activity stream + tool-call rows.

import type { ActivityRow, ToolCall } from "../../lib/cockpitTypes";

interface Props {
  rows: ActivityRow[];
  inFlightTool: ToolCall | null;
  thinking: boolean;
}

export function ActivityStream({ rows, inFlightTool, thinking }: Props) {
  return (
    <div className="bg-slate-800 rounded-md p-4 mb-3">
      <div className="text-xs font-mono uppercase tracking-wide text-slate-400 mb-2">
        Activity
      </div>

      {thinking && (
        <div className="text-slate-400 text-sm italic mb-2">Thinking…</div>
      )}

      {inFlightTool && (
        <div className="rounded bg-slate-900 px-3 py-2 mb-2 border border-amber-600/30">
          <div className="flex items-center gap-2">
            <span className="text-amber-400 text-xs uppercase">running</span>
            <span className="text-slate-100">{inFlightTool.name}</span>
          </div>
          <pre className="font-mono text-xs text-slate-400 mt-1 truncate">
            {inFlightTool.args_preview}
          </pre>
        </div>
      )}

      <ul className="space-y-1">
        {rows.length === 0 && (
          <li className="text-slate-500 text-sm italic">No events yet.</li>
        )}
        {rows
          .slice()
          .reverse()
          .map((row) => (
            <li key={row.id} className="flex items-start gap-2 text-sm">
              <span className="font-mono text-xs text-slate-600 mt-0.5">
                {formatTime(row.at)}
              </span>
              <span className={kindClass(row.kind)}>{kindGlyph(row.kind)}</span>
              <span className="text-slate-300 break-words">{row.text}</span>
            </li>
          ))}
      </ul>
    </div>
  );
}

function kindGlyph(kind: ActivityRow["kind"]): string {
  switch (kind) {
    case "tool_start":
      return "→";
    case "tool_complete":
      return "✓";
    case "tool_error":
      return "✗";
    case "thinking":
      return "…";
    case "message":
    default:
      return "▸";
  }
}

function kindClass(kind: ActivityRow["kind"]): string {
  switch (kind) {
    case "tool_complete":
      return "text-emerald-400";
    case "tool_error":
      return "text-red-400";
    case "tool_start":
      return "text-amber-400";
    case "thinking":
      return "text-slate-400";
    case "message":
    default:
      return "text-teal-400";
  }
}

function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  } catch {
    return "—";
  }
}
