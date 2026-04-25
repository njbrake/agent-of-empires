// Top-level cockpit view: assembles ApprovalCard / PlanPanel /
// ActivityStream and the connection chrome.

import { useCockpit, type ConnectionStatus } from "../../hooks/useCockpit";
import { ApprovalCard } from "./ApprovalCard";
import { PlanPanel } from "./PlanPanel";
import { ActivityStream } from "./ActivityStream";

interface Props {
  sessionId: string;
}

export function CockpitView({ sessionId }: Props) {
  const { state, status, resolveApproval } = useCockpit(sessionId);

  return (
    <div className="flex flex-col h-full overflow-y-auto p-4 bg-slate-900">
      <ConnectionChrome status={status} lagged={state.lagged} />

      {state.rateLimit && (
        <div
          role="status"
          className="mb-3 rounded bg-amber-900/40 border border-amber-700 p-3 text-sm text-amber-200"
        >
          Rate-limited ({state.rateLimit.kind}); resets at{" "}
          {new Date(state.rateLimit.resets_at).toLocaleTimeString()}.
        </div>
      )}

      {state.pendingApprovals.length > 0 && (
        <div aria-label="Pending approvals">
          {state.pendingApprovals.map((approval) => (
            <ApprovalCard
              key={approval.nonce}
              approval={approval}
              onResolve={(decision) => resolveApproval(approval.nonce, decision)}
            />
          ))}
        </div>
      )}

      <PlanPanel plan={state.plan} />

      <ActivityStream
        rows={state.activity}
        inFlightTool={state.inFlightTool}
        thinking={state.thinking}
      />

      {state.assistantMessage && (
        <div className="rounded bg-slate-800 p-3 mb-3 text-slate-200 whitespace-pre-wrap leading-relaxed">
          {state.assistantMessage}
        </div>
      )}
    </div>
  );
}

function ConnectionChrome({
  status,
  lagged,
}: {
  status: ConnectionStatus;
  lagged: boolean;
}) {
  if (status === "open" && !lagged) return null;
  const message =
    status === "connecting"
      ? "Connecting to cockpit…"
      : status === "error"
        ? "Cockpit connection error. Retrying…"
        : status === "closed"
          ? "Cockpit disconnected."
          : lagged
            ? "Reconnected; some events were missed (snapshot recommended)."
            : "";
  if (!message) return null;
  return (
    <div className="mb-3 rounded bg-slate-800 border border-slate-700 p-3 text-sm text-slate-300">
      {message}
    </div>
  );
}
