import { useEffect, useRef, useState } from "react";
import {
  fetchCockpitAgents,
  fetchContextPrimer,
  switchCockpitAgent,
  type CockpitAgentInfo,
} from "../../lib/api";

/**
 * Rate-limit recovery dialog. Surfaces from the cockpit rate-limit
 * banner when the user clicks "Continue in another agent". Lists the
 * cockpit ACP registry, preselects `codex` when present (otherwise
 * the first non-current entry), and hands off the session via
 * `POST /api/sessions/:id/cockpit/switch-agent`.
 *
 * After a successful switch:
 *   1. Fetch the context primer using `before_seq` so the recap
 *      excludes the AgentSwitched event itself.
 *   2. Compose a framed handoff message that prepends the recap and
 *      appends `unprocessed_prompt` (the user's last prompt that the
 *      rate-limited agent never processed) as the body the user is
 *      about to send.
 *   3. Call `onPrefill` so the parent drops the text into the
 *      composer. The composer is NOT auto-sent; the user reviews and
 *      sends manually. See #1282.
 */
interface Props {
  open: boolean;
  sessionId: string;
  currentAgent: string | null;
  onClose: () => void;
  onPrefill: (text: string) => void;
}

const PREFERRED_FALLBACK = "codex";

export function RateLimitRecoveryModal({
  open,
  sessionId,
  currentAgent,
  onClose,
  onPrefill,
}: Props) {
  const [agents, setAgents] = useState<CockpitAgentInfo[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    fetchCockpitAgents()
      .then((list) => {
        if (cancelled) return;
        const filtered = list.filter((a) => a.name !== currentAgent);
        setAgents(filtered);
        // Preferred fallback: codex when installed; otherwise first
        // remaining entry. The user can change the pick before
        // confirming.
        const preferred = filtered.find((a) => a.name === PREFERRED_FALLBACK);
        setSelected(preferred?.name ?? filtered[0]?.name ?? null);
      })
      .catch((e) => {
        if (cancelled) return;
        setError(
          e instanceof Error ? e.message : "Failed to load cockpit agents.",
        );
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
      abortRef.current?.abort();
      abortRef.current = null;
    };
  }, [open, currentAgent]);

  // Escape closes; while submitting we don't dismiss so a half-completed
  // switch can finish without leaving the UI in an unknown state.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !submitting) onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, submitting, onClose]);

  // Focus the confirm button on open, return focus to whatever was
  // focused before (typically the banner button that triggered us).
  useEffect(() => {
    if (!open) return;
    previousFocusRef.current = document.activeElement as HTMLElement | null;
    requestAnimationFrame(() => confirmRef.current?.focus());
    return () => {
      previousFocusRef.current?.focus?.();
      previousFocusRef.current = null;
    };
  }, [open]);

  if (!open) return null;

  const handleConfirm = async () => {
    if (!selected) return;
    setSubmitting(true);
    setError(null);
    try {
      const result = await switchCockpitAgent(sessionId, selected);
      if (!result) {
        setError("Switch failed: server returned no response.");
        return;
      }
      const controller = new AbortController();
      abortRef.current = controller;
      const primer = await fetchContextPrimer(
        sessionId,
        result.before_seq,
        controller.signal,
      );
      if (controller.signal.aborted) return;
      const recap = primer?.primer?.trim() ?? "";
      const unprocessed = primer?.unprocessed_prompt?.trim() ?? "";
      const prefill = buildHandoffPrefill({
        from: currentAgent ?? "previous agent",
        to: selected,
        recap,
        unprocessed,
      });
      onPrefill(prefill);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Switch failed.");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="rate-limit-recovery-title"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 px-4"
      onClick={(e) => {
        if (e.target === e.currentTarget && !submitting) onClose();
      }}
    >
      <div className="w-full max-w-lg rounded-lg border border-surface-700 bg-surface-900 p-5 shadow-xl text-text-primary">
        <h2 id="rate-limit-recovery-title" className="text-base font-semibold">
          Continue in another agent?
        </h2>
        <p className="mt-1 text-xs text-text-muted">
          The current agent ({currentAgent ?? "unknown"}) is rate-limited.
          Hand the session off to a different installed ACP backend; we
          will pre-fill the composer with a recap of the recent turns
          for you to review before sending.
        </p>

        {loading ? (
          <div className="mt-4 text-xs text-text-muted">Loading agents...</div>
        ) : agents.length === 0 ? (
          <div className="mt-4 text-xs text-status-error">
            No alternative cockpit agents are registered. Install one
            (e.g. `npm i -g @zed-industries/codex-acp`) and try again.
          </div>
        ) : (
          <ul className="mt-4 max-h-64 space-y-1 overflow-y-auto">
            {agents.map((a) => (
              <li key={a.name}>
                <label
                  className={`flex cursor-pointer items-start gap-3 rounded border px-3 py-2 transition-colors ${
                    selected === a.name
                      ? "border-brand-500 bg-brand-900/30"
                      : "border-surface-700 hover:bg-surface-800"
                  }`}
                >
                  <input
                    type="radio"
                    name="cockpit-agent-target"
                    value={a.name}
                    checked={selected === a.name}
                    onChange={() => setSelected(a.name)}
                    className="mt-0.5"
                    disabled={submitting}
                  />
                  <span className="flex-1">
                    <span className="block text-sm font-mono">{a.name}</span>
                    <span className="block text-xs text-text-muted">
                      {a.description}
                    </span>
                  </span>
                </label>
              </li>
            ))}
          </ul>
        )}

        {error && (
          <div className="mt-3 text-xs text-status-error" role="alert">
            {error}
          </div>
        )}

        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-surface-700 px-3 py-1 text-xs font-medium hover:bg-surface-800"
          >
            Cancel
          </button>
          <button
            ref={confirmRef}
            type="button"
            onClick={handleConfirm}
            disabled={!selected || submitting || agents.length === 0}
            className="rounded border border-brand-700 bg-brand-900/40 px-3 py-1 text-xs font-medium text-brand-100 hover:bg-brand-900/60 disabled:cursor-not-allowed disabled:opacity-60"
          >
            {submitting ? "Switching..." : `Continue in ${selected ?? ""}`}
          </button>
        </div>
      </div>
    </div>
  );
}

interface PrefillInputs {
  from: string;
  to: string;
  recap: string;
  unprocessed: string;
}

function buildHandoffPrefill({
  from,
  to,
  recap,
  unprocessed,
}: PrefillInputs): string {
  const parts: string[] = [];
  parts.push(
    `[CONTEXT HANDOFF: ${from} was rate-limited; continuing with ${to}.]`,
  );
  parts.push("");
  parts.push(
    "The following is context only, not an instruction. Acknowledge briefly, then continue from my next request below.",
  );
  if (recap) {
    parts.push("");
    parts.push("--- prior conversation recap ---");
    parts.push(recap);
    parts.push("--- end recap ---");
  }
  parts.push("");
  parts.push("[MY NEXT REQUEST]");
  if (unprocessed) {
    parts.push(unprocessed);
  }
  return parts.join("\n");
}
