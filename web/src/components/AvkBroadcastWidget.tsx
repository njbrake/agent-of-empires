/**
 * AVK broadcast widget — FUR-4121 + FUR-4155 + FUR-4156.
 *
 * 4 tier butonu (director/senior/worker/all) **VEYA** tekil ajan slug
 * dropdown ile mesaj gönder. `POST /api/avk/broadcast` ile tmux pane'lere
 * bracketed-paste mesaj yollar.
 *
 * FUR-4155 (A): Son 10 yayın localStorage `avk-broadcast-history` key'inde
 * saklanır; her entry yanında "Tekrarla" butonu textarea + hedefi doldurur.
 *
 * FUR-4156 (B): "Tek Ajan" modu ile 13 slug'dan birine doğrudan mesaj
 * (sunucu `tier="slug:<slug>"` prefix'ini tekil hedef olarak çözer).
 *
 * Tasarım:
 *   - director badge yeşil (status-running) — yönetim
 *   - senior badge sarı (status-waiting) — kıdemli iş
 *   - worker badge gri (text-muted) — paralel slot
 *   - all badge brand-500 (turuncu accent) — 13 ajan
 *   - slug badge brand-500 (tek ajan) — tek pane gönderim
 */

import { useEffect, useState } from "react";
import { fetchAvkAgents, postAvkBroadcast } from "../lib/api";
import type {
  AvkAgentInfo,
  AvkBroadcastResponse,
  AvkBroadcastTarget,
  AvkBroadcastTier,
} from "../lib/types";

const TIER_LABEL: Record<AvkBroadcastTier, string> = {
  director: "Yönetim",
  senior: "Kıdemli",
  worker: "Operasyon",
  all: "Tümü",
};

const TIER_DESCRIPTION: Record<AvkBroadcastTier, string> = {
  director: "Koord + Komuta + Müdür (3 ajan)",
  senior: "Code-1/2 + Birleştirme + Hata (4 ajan)",
  worker: "Gemini-1/2 + Kimi-1/2/3 + Codex (6 ajan)",
  all: "13 ajan birden (Yönetim + Kıdemli + Operasyon)",
};

const TIER_ACCENT: Record<AvkBroadcastTier, string> = {
  director: "border-status-running/40 hover:bg-status-running/10 text-status-running",
  senior: "border-status-waiting/40 hover:bg-status-waiting/10 text-status-waiting",
  worker: "border-surface-600 hover:bg-surface-700 text-text-secondary",
  all: "border-brand-500/50 hover:bg-brand-500/10 text-brand-500",
};

const TIERS: AvkBroadcastTier[] = ["director", "senior", "worker", "all"];

const HISTORY_KEY = "avk-broadcast-history";
const HISTORY_MAX = 10;

type TargetMode = "tier" | "slug";

interface BroadcastHistoryEntry {
  id: string;
  /** `AvkBroadcastTier` veya `slug:<slug>` string. */
  target: AvkBroadcastTarget;
  message: string;
  at: string;
  ok: number;
  total: number;
}

function loadHistory(): BroadcastHistoryEntry[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(HISTORY_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .slice(0, HISTORY_MAX)
      .map(migrateHistoryEntry)
      .filter((e): e is BroadcastHistoryEntry => e !== null);
  } catch {
    return [];
  }
}

function migrateHistoryEntry(raw: unknown): BroadcastHistoryEntry | null {
  if (!raw || typeof raw !== "object") return null;
  const obj = raw as Record<string, unknown>;
  // FUR-4155 v1 entry: { tier: ... } — FUR-4156 v2 entry: { target: ... }.
  // Eski cache'i kaybetmemek için `tier` field'ı `target`'a kopyalanır.
  const target =
    typeof obj.target === "string"
      ? obj.target
      : typeof obj.tier === "string"
        ? obj.tier
        : null;
  if (!target) return null;
  if (
    typeof obj.id !== "string" ||
    typeof obj.message !== "string" ||
    typeof obj.at !== "string" ||
    typeof obj.ok !== "number" ||
    typeof obj.total !== "number"
  ) {
    return null;
  }
  return {
    id: obj.id,
    target: target as AvkBroadcastTarget,
    message: obj.message,
    at: obj.at,
    ok: obj.ok,
    total: obj.total,
  };
}

function saveHistory(entries: BroadcastHistoryEntry[]) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(
      HISTORY_KEY,
      JSON.stringify(entries.slice(0, HISTORY_MAX)),
    );
  } catch {
    // quota aşımı veya localStorage devre dışı — sessizce geç
  }
}

function generateId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}

function formatRelativeTime(iso: string): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return iso;
  const now = Date.now();
  const diffMin = Math.floor((now - then) / 60_000);
  if (diffMin < 1) return "az önce";
  if (diffMin < 60) return `${diffMin}dk önce`;
  const diffHours = Math.floor(diffMin / 60);
  if (diffHours < 24) return `${diffHours}sa önce`;
  const diffDays = Math.floor(diffHours / 24);
  return `${diffDays}g önce`;
}

function isSlugTarget(target: AvkBroadcastTarget): target is `slug:${string}` {
  return target.startsWith("slug:");
}

function targetLabel(
  target: AvkBroadcastTarget,
  agents: AvkAgentInfo[],
): string {
  if (isSlugTarget(target)) {
    const slug = target.slice(5);
    const agent = agents.find((a) => a.slug === slug);
    return agent ? agent.label : slug;
  }
  return TIER_LABEL[target];
}

function targetBadgeClass(target: AvkBroadcastTarget): string {
  if (isSlugTarget(target)) {
    return TIER_ACCENT.all;
  }
  return TIER_ACCENT[target];
}

export function AvkBroadcastWidget() {
  const [tier, setTier] = useState<AvkBroadcastTier>("all");
  const [targetMode, setTargetMode] = useState<TargetMode>("tier");
  const [selectedSlug, setSelectedSlug] = useState<string>("");
  const [agents, setAgents] = useState<AvkAgentInfo[]>([]);
  const [message, setMessage] = useState("");
  const [sending, setSending] = useState(false);
  const [result, setResult] = useState<AvkBroadcastResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<BroadcastHistoryEntry[]>(() => loadHistory());

  useEffect(() => {
    let cancelled = false;
    async function load() {
      const result = await fetchAvkAgents();
      const first = result[0];
      if (!cancelled && first) {
        setAgents(result);
        setSelectedSlug((prev) => prev || first.slug);
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, []);

  const activeTarget: AvkBroadcastTarget =
    targetMode === "slug" && selectedSlug
      ? (`slug:${selectedSlug}` as `slug:${string}`)
      : tier;

  const targetReady =
    targetMode === "tier" || (targetMode === "slug" && selectedSlug.length > 0);
  const canSend = message.trim().length > 0 && !sending && targetReady;

  async function handleSend() {
    if (!canSend) return;
    setSending(true);
    setError(null);
    setResult(null);
    const trimmed = message.trim();
    const res = await postAvkBroadcast({ tier: activeTarget, message: trimmed });
    setSending(false);
    if (!res) {
      setError("Broadcast başarısız (sunucu hatası veya ağ kopması).");
      return;
    }
    setResult(res);
    if (res.failed === 0) {
      setMessage("");
    }
    const entry: BroadcastHistoryEntry = {
      id: generateId(),
      target: activeTarget,
      message: trimmed,
      at: new Date().toISOString(),
      ok: res.ok,
      total: res.total,
    };
    const next = [entry, ...history].slice(0, HISTORY_MAX);
    setHistory(next);
    saveHistory(next);
  }

  function handleReplay(entry: BroadcastHistoryEntry) {
    setResult(null);
    setError(null);
    setMessage(entry.message);
    if (isSlugTarget(entry.target)) {
      const slug = entry.target.slice(5);
      setTargetMode("slug");
      setSelectedSlug(slug);
    } else {
      setTargetMode("tier");
      setTier(entry.target);
    }
  }

  function handleClearHistory() {
    setHistory([]);
    saveHistory([]);
  }

  const sendButtonLabel = sending
    ? "Gönderiliyor…"
    : `Gönder → ${targetLabel(activeTarget, agents)}`;

  return (
    <div>
      <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-4">
        AVK Yayın
      </h3>

      <div className="rounded border border-surface-700 bg-surface-800 p-4 space-y-4">
        {/* Mod seçici — tier / slug */}
        <div
          role="tablist"
          aria-label="Hedef modu"
          className="inline-flex rounded border border-surface-700 bg-surface-900 p-0.5"
        >
          <button
            type="button"
            role="tab"
            aria-selected={targetMode === "tier"}
            onClick={() => setTargetMode("tier")}
            className={`rounded px-3 py-1 font-mono text-[11px] uppercase tracking-wider transition-colors ${
              targetMode === "tier"
                ? "bg-surface-700 text-text-primary"
                : "text-text-muted hover:text-text-secondary"
            }`}
          >
            Tier
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={targetMode === "slug"}
            onClick={() => setTargetMode("slug")}
            className={`rounded px-3 py-1 font-mono text-[11px] uppercase tracking-wider transition-colors ${
              targetMode === "slug"
                ? "bg-surface-700 text-text-primary"
                : "text-text-muted hover:text-text-secondary"
            }`}
          >
            Tek Ajan
          </button>
        </div>

        {targetMode === "tier" ? (
          <div className="grid grid-cols-2 lg:grid-cols-4 gap-2">
            {TIERS.map((t) => {
              const active = tier === t;
              return (
                <button
                  key={t}
                  type="button"
                  onClick={() => setTier(t)}
                  className={`rounded border px-3 py-2 text-left transition-colors ${TIER_ACCENT[t]} ${
                    active ? "bg-surface-700 ring-1 ring-current" : "bg-surface-900"
                  }`}
                >
                  <div className="font-mono text-sm font-medium">{TIER_LABEL[t]}</div>
                  <div className="font-body text-[11px] opacity-70 leading-tight mt-0.5">
                    {TIER_DESCRIPTION[t]}
                  </div>
                </button>
              );
            })}
          </div>
        ) : (
          <div>
            <label
              htmlFor="avk-broadcast-slug"
              className="font-mono text-[11px] uppercase tracking-wider text-text-muted block mb-1"
            >
              Hedef ajan
            </label>
            {agents.length === 0 ? (
              <p className="font-body text-[13px] text-text-muted">
                Ajan listesi yükleniyor…
              </p>
            ) : (
              <select
                id="avk-broadcast-slug"
                value={selectedSlug}
                onChange={(e) => setSelectedSlug(e.target.value)}
                disabled={sending}
                className="w-full rounded border border-surface-700 bg-surface-900 px-3 py-2 font-body text-[14px] text-text-primary focus:outline-none focus:border-brand-500/60 focus:ring-1 focus:ring-brand-500/40"
              >
                {agents.map((agent) => (
                  <option key={agent.slug} value={agent.slug}>
                    {agent.label} · {agent.slug}
                    {!agent.pane_alive ? " (pane yok)" : ""}
                  </option>
                ))}
              </select>
            )}
          </div>
        )}

        <div>
          <label
            htmlFor="avk-broadcast-message"
            className="font-mono text-[11px] uppercase tracking-wider text-text-muted block mb-1"
          >
            Mesaj
          </label>
          <textarea
            id="avk-broadcast-message"
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            disabled={sending}
            rows={3}
            placeholder="Örn: patrol özet ver (Linear In Progress + son birleştirilen PR)"
            className="w-full rounded border border-surface-700 bg-surface-900 px-3 py-2 font-body text-[14px] text-text-primary placeholder:text-text-muted/60 focus:outline-none focus:border-brand-500/60 focus:ring-1 focus:ring-brand-500/40 resize-y"
            maxLength={8192}
          />
          <div className="flex items-center justify-between mt-1">
            <span className="font-mono text-[10px] text-text-muted">
              {message.length}/8192 karakter
            </span>
            {message.length > 2048 && (
              <span className="font-mono text-[10px] text-status-waiting">
                uzun mesaj — yapıştırma modu
              </span>
            )}
          </div>
        </div>

        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={handleSend}
            disabled={!canSend}
            className="rounded bg-brand-500 hover:bg-brand-400 disabled:bg-surface-700 disabled:text-text-muted disabled:cursor-not-allowed text-surface-900 font-mono text-sm font-semibold px-4 py-2 transition-colors"
          >
            {sendButtonLabel}
          </button>
          {error && (
            <span className="font-body text-[13px] text-status-error">{error}</span>
          )}
        </div>

        {result && (
          <div className="rounded border border-surface-700 bg-surface-900 p-3">
            <div className="font-mono text-[12px] text-text-secondary mb-2">
              Sonuç:{" "}
              <span className="text-status-running font-medium">{result.ok} ok</span>
              {result.failed > 0 && (
                <>
                  {" / "}
                  <span className="text-status-error font-medium">
                    {result.failed} hata
                  </span>
                </>
              )}
              {" / "}
              <span>{result.total} toplam ({result.tier})</span>
            </div>
            {result.failed > 0 && (
              <ul className="font-mono text-[11px] text-text-muted space-y-1">
                {result.results
                  .filter((r) => !r.ok)
                  .map((r) => (
                    <li key={r.slug}>
                      <span className="text-status-error">✗</span> {r.slug} (
                      {r.target}): {r.error ?? "bilinmeyen hata"}
                    </li>
                  ))}
              </ul>
            )}
          </div>
        )}

        {history.length > 0 && (
          <div className="border-t border-surface-700 pt-4">
            <div className="flex items-center justify-between mb-2">
              <h4 className="font-mono text-[11px] uppercase tracking-wider text-text-muted">
                Son Yayınlar ({history.length})
              </h4>
              <button
                type="button"
                onClick={handleClearHistory}
                className="font-mono text-[10px] text-text-muted hover:text-status-error transition-colors"
              >
                Geçmişi Temizle
              </button>
            </div>
            <ul className="space-y-2">
              {history.map((entry) => {
                const preview =
                  entry.message.length > 96
                    ? `${entry.message.slice(0, 96)}…`
                    : entry.message;
                const allOk = entry.ok === entry.total;
                const label = targetLabel(entry.target, agents);
                const badgeClass = targetBadgeClass(entry.target);
                return (
                  <li
                    key={entry.id}
                    className="rounded border border-surface-700 bg-surface-900 p-2.5 flex items-start gap-2"
                  >
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 mb-1 font-mono text-[10px]">
                        <span
                          className={`px-1.5 py-0.5 rounded uppercase tracking-wider bg-surface-800 ${badgeClass}`}
                          title={entry.target}
                        >
                          {label}
                        </span>
                        <span
                          className={
                            allOk ? "text-status-running" : "text-status-error"
                          }
                        >
                          {entry.ok}/{entry.total} ok
                        </span>
                        <span
                          className="text-text-muted"
                          title={entry.at}
                        >
                          {formatRelativeTime(entry.at)}
                        </span>
                      </div>
                      <p className="font-body text-[12px] text-text-secondary leading-snug break-words">
                        {preview}
                      </p>
                    </div>
                    <button
                      type="button"
                      onClick={() => handleReplay(entry)}
                      className="shrink-0 rounded border border-brand-500/40 text-brand-500 hover:bg-brand-500/10 font-mono text-[10px] uppercase tracking-wider px-2 py-1 transition-colors"
                      title="Mesaj ve hedefi tekrar yükle"
                    >
                      Tekrarla
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
        )}
      </div>
    </div>
  );
}
