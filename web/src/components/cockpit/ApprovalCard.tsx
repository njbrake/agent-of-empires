// Approval card. Renders a pending tool-call approval with the
// destructive-vs-benign UX from the design spike:
// - Benign: single tap on a primary button.
// - Destructive: long-press 800ms with a haptic confirm; swipe is
//   reserved for dismiss-only and never approves.
//
// The optimistic state (after a tap) shows a spinner + greyed label
// until the server's broadcast removes the approval from state. If the
// remote rejects, the card flips to the rolled-back state.

import { useCallback, useEffect, useRef, useState } from "react";
import type { Approval, ApprovalDecision } from "../../lib/cockpitTypes";

interface Props {
  approval: Approval;
  onResolve: (decision: ApprovalDecision) => Promise<void>;
}

const LONG_PRESS_MS = 800;

export function ApprovalCard({ approval, onResolve }: Props) {
  const [phase, setPhase] = useState<"pending" | "submitting" | "rolled-back">(
    "pending",
  );
  const [progress, setProgress] = useState(0);
  const pressTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const progressTimer = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    return () => {
      if (pressTimer.current) clearTimeout(pressTimer.current);
      if (progressTimer.current) clearInterval(progressTimer.current);
    };
  }, []);

  const submit = useCallback(
    async (decision: ApprovalDecision) => {
      setPhase("submitting");
      try {
        await onResolve(decision);
      } catch {
        setPhase("rolled-back");
      }
    },
    [onResolve],
  );

  const startLongPress = () => {
    if (phase !== "pending") return;
    setProgress(0);
    progressTimer.current = setInterval(() => {
      setProgress((p) => Math.min(100, p + (100 / LONG_PRESS_MS) * 30));
    }, 30);
    pressTimer.current = setTimeout(() => {
      if (progressTimer.current) {
        clearInterval(progressTimer.current);
        progressTimer.current = null;
      }
      if (typeof navigator !== "undefined" && "vibrate" in navigator) {
        try {
          (navigator as Navigator & { vibrate?: (p: number) => void }).vibrate?.(
            20,
          );
        } catch {
          // ignore
        }
      }
      void submit("Allow");
    }, LONG_PRESS_MS);
  };

  const cancelLongPress = () => {
    if (pressTimer.current) {
      clearTimeout(pressTimer.current);
      pressTimer.current = null;
    }
    if (progressTimer.current) {
      clearInterval(progressTimer.current);
      progressTimer.current = null;
    }
    setProgress(0);
  };

  const borderClass = approval.destructive
    ? "border-l-4 border-red-500"
    : "border-l-4 border-teal-600";

  return (
    <div
      className={`rounded-md bg-surface-800 p-4 mb-3 shadow-md ${borderClass}`}
      role="alertdialog"
      aria-label={`Approval needed: ${approval.tool_call.name}`}
    >
      <div className="flex items-center gap-2 mb-2">
        {approval.destructive && (
          <span className="text-red-400 text-xs uppercase tracking-wide font-semibold">
            destructive
          </span>
        )}
        <span className="text-text-primary font-medium">
          {approval.tool_call.name}
        </span>
      </div>

      <pre className="font-mono text-xs text-text-secondary bg-surface-900 rounded p-2 mb-3 overflow-x-auto">
        {approval.tool_call.args_preview}
      </pre>

      {phase === "rolled-back" && (
        <p className="text-red-400 text-sm mb-2">
          Could not reach the server. Tap to retry.
        </p>
      )}

      <div className="flex items-stretch gap-2">
        {approval.destructive ? (
          <button
            type="button"
            className={`relative flex-1 rounded text-white font-medium py-3 px-4 text-sm overflow-hidden ${
              phase === "pending"
                ? "bg-red-600 hover:bg-red-500"
                : "bg-red-700 opacity-70 cursor-wait"
            }`}
            disabled={phase !== "pending" && phase !== "rolled-back"}
            onMouseDown={startLongPress}
            onMouseUp={cancelLongPress}
            onMouseLeave={cancelLongPress}
            onTouchStart={startLongPress}
            onTouchEnd={cancelLongPress}
            onTouchCancel={cancelLongPress}
          >
            <span className="relative z-10">
              {phase === "submitting"
                ? "Approving…"
                : `Hold to allow (${LONG_PRESS_MS}ms)`}
            </span>
            <span
              className="absolute inset-0 bg-red-400 origin-left"
              style={{ transform: `scaleX(${progress / 100})` }}
              aria-hidden="true"
            />
          </button>
        ) : (
          <button
            type="button"
            className={`flex-1 rounded text-white font-medium py-3 px-4 text-sm ${
              phase === "pending"
                ? "bg-brand-600 hover:bg-brand-500"
                : "bg-brand-700 opacity-70 cursor-wait"
            }`}
            disabled={phase !== "pending"}
            onClick={() => void submit("Allow")}
          >
            {phase === "submitting" ? "Approving…" : "Allow"}
          </button>
        )}

        <button
          type="button"
          className="rounded bg-surface-700 hover:bg-surface-700 text-text-primary font-medium py-3 px-4 text-sm"
          disabled={phase === "submitting"}
          onClick={() => void submit("Deny")}
        >
          Deny
        </button>
      </div>
    </div>
  );
}
