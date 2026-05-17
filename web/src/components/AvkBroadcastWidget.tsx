/**
 * AVK broadcast widget — FUR-4121.
 *
 * 4 tier butonu (director/senior/worker/all) + mesaj textarea + Gönder.
 * `POST /api/avk/broadcast` ile tmux pane'lere bracketed-paste mesaj yollar.
 * Sonuç inline summary (ok / failed) ve gerekirse per-pane hata listesi.
 *
 * Tasarım:
 *   - director badge yeşil (status-running) — yönetim
 *   - senior badge sarı (status-waiting) — kıdemli iş
 *   - worker badge gri (text-muted) — paralel slot
 *   - all badge brand-500 (turuncu accent) — 13 ajan
 *
 * Mobile-first: 1 col tier seçim grid, lg breakpoint 4 col yan yana.
 */

import { useState } from "react";
import { postAvkBroadcast } from "../lib/api";
import type {
  AvkBroadcastResponse,
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

export function AvkBroadcastWidget() {
  const [tier, setTier] = useState<AvkBroadcastTier>("all");
  const [message, setMessage] = useState("");
  const [sending, setSending] = useState(false);
  const [result, setResult] = useState<AvkBroadcastResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  const canSend = message.trim().length > 0 && !sending;

  async function handleSend() {
    if (!canSend) return;
    setSending(true);
    setError(null);
    setResult(null);
    const res = await postAvkBroadcast({ tier, message: message.trim() });
    setSending(false);
    if (!res) {
      setError("Broadcast başarısız (sunucu hatası veya ağ kopması).");
      return;
    }
    setResult(res);
    if (res.failed === 0) {
      setMessage("");
    }
  }

  return (
    <div>
      <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-4">
        AVK Yayın
      </h3>

      <div className="rounded border border-surface-700 bg-surface-800 p-4 space-y-4">
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
            {sending ? "Gönderiliyor…" : `Gönder → ${TIER_LABEL[tier]}`}
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
      </div>
    </div>
  );
}
