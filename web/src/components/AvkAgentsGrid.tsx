/**
 * AVK workflow ajan grid widget — FUR-3957 transplant Adım 7.
 *
 * `GET /api/avk/agents` server endpoint'inden 13 ajan kaydını çeker;
 * Director / Senior / Worker tier sıralı 3 grup card grid render eder.
 *
 * `tmux_target` runtime resync gerektirir (pane index değişir) — bu
 * yüzden komponent mount + 60sn refresh interval ile yeniden fetch eder.
 *
 * Slug → label canonical kaynak server `src/avk_agents.rs::AVK_AGENTS`.
 * Tier badge rengi:
 *   - director → status-running (yeşil) — sistem yönetir
 *   - senior   → status-idle    (sarı)  — kıdemli iş
 *   - worker   → text-muted    (gri)  — paralel slot
 */

import { useEffect, useState } from "react";
import { fetchAvkAgents } from "../lib/api";
import type { AvkAgentInfo, AvkAgentRole } from "../lib/types";

const REFRESH_INTERVAL_MS = 60_000;

const ROLE_ORDER: AvkAgentRole[] = ["director", "senior", "worker"];

const ROLE_LABEL: Record<AvkAgentRole, string> = {
  director: "Director",
  senior: "Senior",
  worker: "Worker",
};

const ROLE_BADGE_CLASS: Record<AvkAgentRole, string> = {
  director: "bg-status-running/15 text-status-running",
  senior: "bg-status-idle/15 text-status-idle",
  worker: "bg-surface-700 text-text-muted",
};

function groupByRole(agents: AvkAgentInfo[]): Record<AvkAgentRole, AvkAgentInfo[]> {
  const grouped: Record<AvkAgentRole, AvkAgentInfo[]> = {
    director: [],
    senior: [],
    worker: [],
  };
  for (const agent of agents) {
    grouped[agent.role].push(agent);
  }
  return grouped;
}

export function AvkAgentsGrid() {
  const [agents, setAgents] = useState<AvkAgentInfo[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      const result = await fetchAvkAgents();
      if (!cancelled) {
        setAgents(result);
        setLoading(false);
      }
    }

    load();
    const id = setInterval(load, REFRESH_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  if (loading) {
    return (
      <div>
        <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-4">
          AVK Workflow Agents
        </h3>
        <p className="font-body text-[14px] text-text-muted">Loading…</p>
      </div>
    );
  }

  if (agents.length === 0) {
    return (
      <div>
        <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-4">
          AVK Workflow Agents
        </h3>
        <p className="font-body text-[14px] text-text-muted">
          No agents available (server `/api/avk/agents` returned empty).
        </p>
      </div>
    );
  }

  const grouped = groupByRole(agents);

  return (
    <div>
      <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-4">
        AVK Workflow Agents ({agents.length})
      </h3>
      <div className="space-y-6">
        {ROLE_ORDER.map((role) => {
          const tierAgents = grouped[role];
          if (tierAgents.length === 0) return null;
          return (
            <section key={role}>
              <h4 className="font-mono text-xs uppercase tracking-wider text-text-muted mb-2">
                {ROLE_LABEL[role]} ({tierAgents.length})
              </h4>
              <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-2">
                {tierAgents.map((agent) => (
                  <article
                    key={agent.slug}
                    className="rounded border border-surface-700 bg-surface-800 p-3"
                  >
                    <div className="flex items-center justify-between mb-1">
                      <span className="font-body text-[14px] font-medium">
                        {agent.label}
                      </span>
                      <span
                        className={`font-mono text-[10px] uppercase tracking-wider px-2 py-0.5 rounded ${ROLE_BADGE_CLASS[agent.role]}`}
                      >
                        {agent.role}
                      </span>
                    </div>
                    <div className="flex items-center justify-between font-mono text-[11px] text-text-muted">
                      <span>{agent.slug}</span>
                      <span title={agent.tmux_target}>{agent.tmux_target}</span>
                    </div>
                  </article>
                ))}
              </div>
            </section>
          );
        })}
      </div>
    </div>
  );
}
