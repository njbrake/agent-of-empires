/**
 * AVK workflow ajan grid widget — FUR-3957 transplant Adım 7 + FUR-4123 polish.
 *
 * `GET /api/avk/agents` server endpoint'inden 13 ajan kaydını çeker;
 * Director / Senior / Worker tier sıralı 3 grup card grid render eder.
 *
 * `tmux_target` registry sabit (avk-ofis:...), `runtime_target` AoE
 * binary'nin gerçek yarattığı session (aoe_<slug>_<hash>:^.0). Server
 * resolver (FUR-4122) her ajan için canlı pane durumunu döner —
 * `pane_alive=true` yeşil dot, false gri dot. 30s refresh interval.
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

const REFRESH_INTERVAL_MS = 30_000;

const ROLE_ORDER: AvkAgentRole[] = ["director", "senior", "worker"];

const ROLE_LABEL: Record<AvkAgentRole, string> = {
  director: "Yönetim",
  senior: "Kıdemli",
  worker: "Operasyon",
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
          AVK İş Ajanları
        </h3>
        <p className="font-body text-[14px] text-text-muted">Yükleniyor…</p>
      </div>
    );
  }

  if (agents.length === 0) {
    return (
      <div>
        <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-4">
          AVK İş Ajanları
        </h3>
        <p className="font-body text-[14px] text-text-muted">
          Ajan bulunamadı (sunucu `/api/avk/agents` boş döndü).
        </p>
      </div>
    );
  }

  const grouped = groupByRole(agents);
  const aliveCount = agents.filter((a) => a.pane_alive).length;

  return (
    <div>
      <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-4">
        AVK İş Ajanları ({agents.length}){" "}
        <span className="text-status-running normal-case tracking-normal">
          · {aliveCount}/{agents.length} canlı
        </span>
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
                {tierAgents.map((agent) => {
                  const effectiveTarget = agent.runtime_target ?? agent.tmux_target;
                  const dotClass = agent.pane_alive
                    ? "bg-status-running"
                    : "bg-surface-600";
                  const dotTitle = agent.pane_alive
                    ? `canlı · ${effectiveTarget}`
                    : `pane yok · kayıt: ${agent.tmux_target}`;
                  return (
                    <article
                      key={agent.slug}
                      className="rounded border border-surface-700 bg-surface-800 p-3"
                    >
                      <div className="flex items-center justify-between mb-1">
                        <div className="flex items-center gap-2 min-w-0">
                          <span
                            className={`inline-block w-2 h-2 rounded-full shrink-0 ${dotClass}`}
                            title={dotTitle}
                            aria-label={dotTitle}
                          />
                          <span className="font-body text-[14px] font-medium truncate">
                            {agent.label}
                          </span>
                        </div>
                        <span
                          className={`font-mono text-[10px] uppercase tracking-wider px-2 py-0.5 rounded shrink-0 ${ROLE_BADGE_CLASS[agent.role]}`}
                        >
                          {agent.role}
                        </span>
                      </div>
                      <div className="flex items-center justify-between font-mono text-[11px] text-text-muted gap-2">
                        <span className="shrink-0">{agent.slug}</span>
                        <span
                          className="truncate"
                          title={`kayıt: ${agent.tmux_target}${agent.runtime_target ? ` · çalışan: ${agent.runtime_target}` : ""}`}
                        >
                          {effectiveTarget}
                        </span>
                      </div>
                    </article>
                  );
                })}
              </div>
            </section>
          );
        })}
      </div>
    </div>
  );
}
