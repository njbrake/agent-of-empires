// Plan panel. Renders the agent's structured plan with sticky
// current-step at the top and collapsed completed steps below.

import { useState } from "react";
import type { Plan } from "../../lib/cockpitTypes";

interface Props {
  plan: Plan | null;
}

export function PlanPanel({ plan }: Props) {
  const [showCompleted, setShowCompleted] = useState(false);
  const [showUpcoming, setShowUpcoming] = useState(true);

  if (!plan) {
    return (
      <div className="bg-slate-800 rounded-md p-4 mb-3 text-slate-400 text-sm italic">
        Agent is planning…
      </div>
    );
  }

  const current = plan.steps.find((s) => s.status === "InProgress");
  const completed = plan.steps.filter((s) => s.status === "Done");
  const upcoming = plan.steps.filter(
    (s) => s.status === "Pending" || s.status === "Cancelled",
  );

  return (
    <div className="bg-slate-800 rounded-md p-4 mb-3">
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs font-mono uppercase tracking-wide text-slate-400">
          Plan · v{plan.version}
        </span>
        <span className="text-xs text-slate-500">{plan.steps.length} steps</span>
      </div>

      {current && (
        <div className="rounded bg-slate-900 p-3 mb-3 border border-amber-600/40">
          <div className="text-amber-400 text-xs uppercase tracking-wide mb-1">
            current
          </div>
          <div className="text-slate-100 font-medium">{current.title}</div>
          {current.detail && (
            <div className="text-slate-400 text-sm mt-1">{current.detail}</div>
          )}
        </div>
      )}

      <button
        type="button"
        className="text-xs text-slate-400 hover:text-slate-200 mb-1"
        onClick={() => setShowUpcoming((v) => !v)}
      >
        {showUpcoming ? "▾" : "▸"} Upcoming ({upcoming.length})
      </button>
      {showUpcoming && (
        <ul className="text-sm text-slate-300 ml-2">
          {upcoming.map((step) => (
            <li
              key={step.id}
              className="border-l-2 border-slate-700 pl-2 mb-1"
            >
              {step.status === "Cancelled" && (
                <span className="text-slate-500 line-through">{step.title}</span>
              )}
              {step.status === "Pending" && step.title}
            </li>
          ))}
        </ul>
      )}

      <button
        type="button"
        className="text-xs text-slate-400 hover:text-slate-200 mt-3 mb-1"
        onClick={() => setShowCompleted((v) => !v)}
      >
        {showCompleted ? "▾" : "▸"} Completed ({completed.length})
      </button>
      {showCompleted && (
        <ul className="text-sm text-slate-500 ml-2">
          {completed.map((step) => (
            <li key={step.id} className="pl-2 mb-1">
              ✓ {step.title}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
