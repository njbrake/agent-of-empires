/**
 * AVK workflow ajan grid widget — FUR-3957 transplant Adım 7 + FUR-4123 polish
 * + FUR-4161 pane peek.
 *
 * `GET /api/avk/agents` server endpoint'inden 13 ajan kaydını çeker;
 * Director / Senior / Worker tier sıralı 3 grup card grid render eder.
 *
 * `tmux_target` registry sabit (avk-ofis:...), `runtime_target` AoE
 * binary'nin gerçek yarattığı session (aoe_<slug>_<hash>:^.0). Server
 * resolver (FUR-4122) her ajan için canlı pane durumunu döner —
 * `pane_alive=true` yeşil dot, false gri dot. 30s refresh interval.
 *
 * FUR-4161: Kart tıklaması ile `GET /api/avk/pane-peek?slug=<slug>&lines=40`
 * çağrılır; son 40 satır monospace pre içinde inline expand olur. Tek seferde
 * bir kart expanded (tekrar tıklama kapatır).
 *
 * Slug → label canonical kaynak server `src/avk_agents.rs::AVK_AGENTS`.
 */

import { useEffect, useRef, useState } from "react";
import {
  fetchAvkAgents,
  fetchAvkPanePeek,
  postAvkBroadcast,
} from "../lib/api";
import type {
  AvkAgentInfo,
  AvkAgentRole,
  AvkPanePeekResponse,
} from "../lib/types";

const REFRESH_INTERVAL_MS = 30_000;
const PEEK_AUTO_REFRESH_MS = 5_000;
const PEEK_LINES = 40;

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

type PeekState =
  | { kind: "loading" }
  | { kind: "ready"; data: AvkPanePeekResponse }
  | { kind: "error" };

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
  const [expandedSlug, setExpandedSlug] = useState<string | null>(null);
  const [peekMap, setPeekMap] = useState<Record<string, PeekState>>({});

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

  async function refreshPeek(slug: string, silent = false) {
    if (!silent) {
      setPeekMap((prev) => ({ ...prev, [slug]: { kind: "loading" } }));
    }
    const result = await fetchAvkPanePeek(slug, PEEK_LINES);
    setPeekMap((prev) => ({
      ...prev,
      [slug]: result ? { kind: "ready", data: result } : { kind: "error" },
    }));
  }

  async function togglePeek(slug: string) {
    if (expandedSlug === slug) {
      setExpandedSlug(null);
      return;
    }
    setExpandedSlug(slug);
    if (!peekMap[slug] || peekMap[slug].kind === "error") {
      await refreshPeek(slug);
    }
  }

  useEffect(() => {
    if (!expandedSlug) return;
    const id = setInterval(() => {
      refreshPeek(expandedSlug, true);
    }, PEEK_AUTO_REFRESH_MS);
    return () => clearInterval(id);
  }, [expandedSlug]);

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
        <span className="ml-2 normal-case tracking-normal text-text-dim text-[11px]">
          · kart tıkla → önizleme
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
                  const expanded = expandedSlug === agent.slug;
                  const peek = peekMap[agent.slug];
                  return (
                    <article
                      key={agent.slug}
                      className={`rounded border bg-surface-800 p-3 transition-colors ${
                        expanded
                          ? "border-brand-500/50"
                          : "border-surface-700 hover:border-surface-600"
                      } ${expanded ? "sm:col-span-2 lg:col-span-3" : ""}`}
                    >
                      <button
                        type="button"
                        onClick={() => togglePeek(agent.slug)}
                        className="w-full text-left cursor-pointer"
                        aria-expanded={expanded}
                        aria-controls={`peek-${agent.slug}`}
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
                      </button>
                      {expanded && (
                        <>
                          <PeekPanel
                            id={`peek-${agent.slug}`}
                            peek={peek}
                            target={effectiveTarget}
                          />
                          <InjectBox
                            slug={agent.slug}
                            label={agent.label}
                            onSent={() => refreshPeek(agent.slug, true)}
                          />
                        </>
                      )}
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

function InjectBox({
  slug,
  label,
  onSent,
}: {
  slug: string;
  label: string;
  onSent: () => void;
}) {
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<
    | { kind: "idle" }
    | { kind: "ok"; ts: number }
    | { kind: "err"; reason: string }
  >({ kind: "idle" });
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  async function handleSend() {
    const trimmed = message.trim();
    if (!trimmed || busy) return;
    setBusy(true);
    setStatus({ kind: "idle" });
    const res = await postAvkBroadcast({
      tier: `slug:${slug}`,
      message: trimmed,
    });
    setBusy(false);
    if (res && res.failed === 0 && res.ok > 0) {
      setStatus({ kind: "ok", ts: Date.now() });
      setMessage("");
      textareaRef.current?.focus();
      setTimeout(() => onSent(), 600);
    } else {
      const reason =
        res?.results?.[0]?.error ??
        (res === null ? "ağ hatası / 4xx" : "bilinmeyen hata");
      setStatus({ kind: "err", reason });
    }
  }

  function handleKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      handleSend();
    }
  }

  return (
    <div className="mt-3 border-t border-surface-700 pt-3">
      <label
        htmlFor={`inject-${slug}`}
        className="block font-mono text-[10px] uppercase tracking-wider text-text-muted mb-1"
      >
        {label} pane'ine mesaj gönder
      </label>
      <textarea
        ref={textareaRef}
        id={`inject-${slug}`}
        value={message}
        onChange={(e) => setMessage(e.target.value)}
        onKeyDown={handleKeyDown}
        placeholder={`@${slug} — Cmd/Ctrl+Enter ile gönder`}
        rows={3}
        className="w-full font-mono text-[12px] bg-surface-900 border border-surface-700 rounded p-2 text-text-secondary focus:border-brand-500/50 focus:outline-none resize-y"
      />
      <div className="flex items-center justify-between mt-2 gap-2">
        <span className="font-mono text-[10px] text-text-dim">
          {message.length}/8192
          {status.kind === "ok" && (
            <span className="ml-2 text-status-running">✓ gönderildi</span>
          )}
          {status.kind === "err" && (
            <span className="ml-2 text-status-error">✗ {status.reason}</span>
          )}
        </span>
        <button
          type="button"
          onClick={handleSend}
          disabled={busy || message.trim().length === 0}
          className="font-mono text-[11px] uppercase tracking-wider px-3 py-1 rounded border border-brand-500/50 text-brand-300 hover:bg-brand-500/10 disabled:opacity-40 disabled:cursor-not-allowed"
        >
          {busy ? "gönderiliyor…" : "Gönder"}
        </button>
      </div>
    </div>
  );
}

function PeekPanel({
  id,
  peek,
  target,
}: {
  id: string;
  peek: PeekState | undefined;
  target: string;
}) {
  if (!peek || peek.kind === "loading") {
    return (
      <div id={id} className="mt-3 border-t border-surface-700 pt-3">
        <p className="font-body text-[12px] text-text-muted">
          Önizleme alınıyor (`tmux capture-pane -t {target} -pS -{PEEK_LINES}`)…
        </p>
      </div>
    );
  }
  if (peek.kind === "error") {
    return (
      <div id={id} className="mt-3 border-t border-surface-700 pt-3">
        <p className="font-body text-[12px] text-status-error">
          Önizleme alınamadı (404 slug bilinmiyor veya tmux capture hata).
        </p>
      </div>
    );
  }
  const text = peek.data.content.replace(/\[[0-9;]*[A-Za-z]/g, "");
  const trimmed = text.trimEnd();
  return (
    <div id={id} className="mt-3 border-t border-surface-700 pt-3">
      <div className="flex items-center justify-between mb-1 font-mono text-[10px] text-text-muted">
        <span>
          son {peek.data.lines} satır ·{" "}
          {peek.data.runtime_resolved ? "runtime" : "kayıt"} ·{" "}
          <span className="text-text-secondary">{peek.data.target}</span>
        </span>
        {trimmed.length === 0 && (
          <span className="text-text-dim">pane sessiz</span>
        )}
      </div>
      {trimmed.length > 0 && (
        <pre className="font-mono text-[11px] leading-snug text-text-secondary bg-surface-900 border border-surface-700 rounded p-2 max-h-72 overflow-auto whitespace-pre-wrap break-words">
          {trimmed}
        </pre>
      )}
    </div>
  );
}
