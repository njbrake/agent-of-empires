/**
 * AVK Ofis kontrol widget — tmux 13 ajan layout başlat/yeniden başlat.
 *
 * Tek buton: "Ofis Başlat". Backend sabit script çağırır
 * (`/root/ajan-sistemi/apps/vps/scripts/avk-ofis-baslat`, idempotent).
 * Tıklama → 30-60s "kuruluyor" indicator → sonuç tail satırları.
 *
 * Furkan canon 2026-05-18: "garanti olsun diye bunu nasıl başlatmam
 * gerektiğini start gibi bir buton ile tarayıcıdaki dashboarda ekler
 * misin" — VPS reboot veya kazara session ölünce tek tık kurtarma.
 */

import { useState } from "react";
import { postAvkOfisBaslat } from "../lib/api";
import type { AvkOfisBaslatResponse } from "../lib/types";

export function AvkOfisControl() {
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<AvkOfisBaslatResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [startedAt, setStartedAt] = useState<number | null>(null);

  async function handleStart() {
    if (running) return;
    if (!confirm("Ofis tmux session 13 ajan layout başlatılsın mı? (mevcut session varsa atlanır)")) return;
    setRunning(true);
    setError(null);
    setResult(null);
    setStartedAt(Date.now());
    try {
      const res = await postAvkOfisBaslat();
      if (!res) {
        setError("Endpoint ulaşılamadı (/api/avk/ofis-baslat)");
      } else {
        setResult(res);
        if (!res.ok) {
          setError(res.error ?? "Script başarısız döndü");
        }
      }
    } finally {
      setRunning(false);
    }
  }

  return (
    <div>
      <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-1">
        Ofis Kontrol
      </h3>
      <p className="font-body text-[12px] text-text-dim mb-3">
        VPS tmux 13 ajan layout'unu başlatır. Mevcut session varsa atlanır
        (idempotent).
      </p>

      <button
        onClick={handleStart}
        disabled={running}
        className="rounded-lg bg-status-running/20 border border-status-running/40 text-status-running font-mono text-[13px] px-4 py-2 hover:bg-status-running/30 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
      >
        {running ? "Kuruluyor… (30-60s)" : "▶ Ofis Başlat"}
      </button>

      {running && startedAt && (
        <p className="font-mono text-[11px] text-text-muted mt-2">
          CLI boot bekleniyor + 7 Claude pane launcher inject…
        </p>
      )}

      {error && !running && (
        <p className="font-body text-[12px] text-status-error mt-3">
          ❌ {error}
        </p>
      )}

      {result && !running && (
        <div className="mt-3 space-y-2">
          <div className="flex items-center gap-3 text-[12px] font-mono">
            <span className={result.ok ? "text-status-running" : "text-status-error"}>
              {result.ok ? "✓ başarılı" : "✗ başarısız"}
            </span>
            <span className="text-text-muted">
              {(result.elapsed_ms / 1000).toFixed(1)}s
            </span>
          </div>
          {result.stdout_tail && (
            <pre className="font-mono text-[11px] text-text-secondary bg-surface-800 border border-surface-700 rounded p-3 overflow-x-auto whitespace-pre-wrap max-h-48">
{result.stdout_tail}
            </pre>
          )}
          {result.stderr_tail && (
            <pre className="font-mono text-[11px] text-status-error bg-surface-800 border border-status-error/40 rounded p-3 overflow-x-auto whitespace-pre-wrap max-h-32">
{result.stderr_tail}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
