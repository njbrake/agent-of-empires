// Top-level cockpit view: assembles ApprovalCard / PlanPanel /
// ActivityStream and the connection chrome.
//
// Layout:
// - mobile (<768px): single-column stack with chat drawer FAB
// - desktop (>=768px): three-pane: plan left, activity center, chat dock right

import { useEffect, useState } from "react";
import { useCockpit, type ConnectionStatus } from "../../hooks/useCockpit";
import { ApprovalCard } from "./ApprovalCard";
import { PlanPanel } from "./PlanPanel";
import { ActivityStream } from "./ActivityStream";
import { ChatDrawer } from "./ChatDrawer";

interface Props {
  sessionId: string;
}

export function CockpitView({ sessionId }: Props) {
  const { state, status, resolveApproval, sendPrompt } = useCockpit(sessionId);
  const isDesktop = useIsDesktop();

  return (
    <div className="flex h-full flex-col md:flex-row bg-slate-900">
      <ConnectionAndApprovals
        status={status}
        lagged={state.lagged}
        rateLimit={state.rateLimit}
        approvals={state.pendingApprovals}
        onResolve={resolveApproval}
      />

      {isDesktop ? (
        <DesktopLayout
          state={state}
          sessionId={sessionId}
          sendPrompt={sendPrompt}
        />
      ) : (
        <MobileLayout
          state={state}
          sessionId={sessionId}
          sendPrompt={sendPrompt}
        />
      )}
    </div>
  );
}

interface MobileLayoutProps {
  state: ReturnType<typeof useCockpit>["state"];
  sessionId: string;
  sendPrompt: (text: string) => Promise<void>;
}

function MobileLayout({ state, sessionId, sendPrompt }: MobileLayoutProps) {
  return (
    <div className="flex flex-col w-full h-full overflow-y-auto p-4">
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

      <ChatDrawer
        sessionId={sessionId}
        onSubmit={sendPrompt}
        variant="mobile"
      />
    </div>
  );
}

function DesktopLayout({ state, sessionId, sendPrompt }: MobileLayoutProps) {
  return (
    <div className="grid grid-cols-[300px_minmax(0,1fr)_360px] w-full h-full overflow-hidden">
      <aside className="overflow-y-auto p-4 border-r border-slate-700">
        <PlanPanel plan={state.plan} />
      </aside>
      <section className="overflow-y-auto p-4">
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
      </section>
      <aside className="h-full">
        <ChatDrawer
          sessionId={sessionId}
          onSubmit={sendPrompt}
          variant="desktop"
        />
      </aside>
    </div>
  );
}

interface ChromeProps {
  status: ConnectionStatus;
  lagged: boolean;
  rateLimit: ReturnType<typeof useCockpit>["state"]["rateLimit"];
  approvals: ReturnType<typeof useCockpit>["state"]["pendingApprovals"];
  onResolve: (
    nonce: string,
    decision: import("../../lib/cockpitTypes").ApprovalDecision,
  ) => Promise<void>;
}

function ConnectionAndApprovals({
  status,
  lagged,
  rateLimit,
  approvals,
  onResolve,
}: ChromeProps) {
  const showChrome = status !== "open" || lagged || !!rateLimit || approvals.length > 0;
  if (!showChrome) return null;
  return (
    <div className="absolute md:static inset-x-0 top-0 z-20 md:z-auto p-4 bg-slate-900/95 backdrop-blur md:bg-transparent md:p-2 md:border-b md:border-slate-700">
      <ConnectionChrome status={status} lagged={lagged} />
      {rateLimit && (
        <div
          role="status"
          className="mb-3 rounded bg-amber-900/40 border border-amber-700 p-3 text-sm text-amber-200"
        >
          Rate-limited ({rateLimit.kind}); resets at{" "}
          {new Date(rateLimit.resets_at).toLocaleTimeString()}.
        </div>
      )}
      {approvals.map((approval) => (
        <ApprovalCard
          key={approval.nonce}
          approval={approval}
          onResolve={(decision) => onResolve(approval.nonce, decision)}
        />
      ))}
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

function useIsDesktop(): boolean {
  const [isDesktop, setIsDesktop] = useState(() => {
    if (typeof window === "undefined") return false;
    return window.matchMedia("(min-width: 768px)").matches;
  });
  useEffect(() => {
    if (typeof window === "undefined") return;
    const mq = window.matchMedia("(min-width: 768px)");
    const handler = (event: MediaQueryListEvent) => setIsDesktop(event.matches);
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, []);
  return isDesktop;
}
