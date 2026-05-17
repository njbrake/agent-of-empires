/**
 * AVK VPS sistem durum widget.
 *
 * `GET /api/avk/vps-status` → daemon host metrikleri (hostname + kernel +
 * uptime + load avg + memory % + disk %). 30s refresh interval —
 * AvkSystemHealth ile aynı kadans. Birden fazla VPS gelecekte
 * eklenebilsin diye liste-tabanlı render (şimdilik 1 host).
 */

import { useEffect, useState } from "react";
import { fetchAvkVpsStatus } from "../lib/api";
import type { AvkVpsStatusResponse } from "../lib/types";

const REFRESH_INTERVAL_MS = 30_000;

function formatUptime(sec: number | null): string {
  if (sec == null) return "—";
  if (sec < 60) return `${sec}sn`;
  if (sec < 3600) return `${Math.floor(sec / 60)}dk`;
  if (sec < 86_400) {
    const h = Math.floor(sec / 3600);
    const m = Math.floor((sec % 3600) / 60);
    return m > 0 ? `${h}sa ${m}dk` : `${h}sa`;
  }
  const d = Math.floor(sec / 86_400);
  const h = Math.floor((sec % 86_400) / 3600);
  return h > 0 ? `${d}g ${h}sa` : `${d}g`;
}

function formatKb(kb: number): string {
  if (kb < 1024) return `${kb}KB`;
  if (kb < 1024 * 1024) return `${(kb / 1024).toFixed(1)}MB`;
  if (kb < 1024 * 1024 * 1024) return `${(kb / 1024 / 1024).toFixed(1)}GB`;
  return `${(kb / 1024 / 1024 / 1024).toFixed(1)}TB`;
}

function pctAccent(pct: number): string {
  if (pct >= 90) return "text-status-error";
  if (pct >= 75) return "text-status-waiting";
  return "text-status-running";
}

function loadAccent(load: number, cpu: number | null): string {
  if (cpu == null) return "text-text-secondary";
  const ratio = load / cpu;
  if (ratio >= 1.5) return "text-status-error";
  if (ratio >= 0.8) return "text-status-waiting";
  return "text-status-running";
}

export function AvkVpsStatus() {
  const [vps, setVps] = useState<AvkVpsStatusResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [stale, setStale] = useState(false);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      const result = await fetchAvkVpsStatus();
      if (cancelled) return;
      if (result) {
        setVps(result);
        setStale(false);
      } else {
        setStale(true);
      }
      setLoading(false);
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
        <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-3">
          VPS Durum
        </h3>
        <p className="font-body text-[13px] text-text-muted">Yükleniyor…</p>
      </div>
    );
  }

  if (!vps) {
    return (
      <div>
        <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-3">
          VPS Durum
        </h3>
        <p className="font-body text-[13px] text-status-error">
          VPS durum endpoint'i ulaşılamadı (`/api/avk/vps-status`).
        </p>
      </div>
    );
  }

  const load1 = vps.load_avg?.[0];
  const load5 = vps.load_avg?.[1];
  const load15 = vps.load_avg?.[2];

  return (
    <div>
      <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-1">
        VPS Durum
        {stale && (
          <span className="ml-2 normal-case tracking-normal text-status-waiting text-[11px]">
            · yenilenemedi
          </span>
        )}
      </h3>
      <p className="font-mono text-[11px] text-text-dim mb-3 truncate">
        <span className="text-text-secondary">{vps.hostname}</span>
        {vps.os && <span> · {vps.os}</span>}
        {vps.kernel && <span> · {vps.kernel}</span>}
      </p>

      <div className="grid grid-cols-2 lg:grid-cols-4 gap-2">
        <StatusBadge
          label="Süre"
          value={formatUptime(vps.uptime_sec)}
          accent="text-text-secondary"
        />
        <StatusBadge
          label="CPU"
          value={vps.cpu_count != null ? `${vps.cpu_count} çekirdek` : "—"}
          accent="text-text-secondary"
        />
        <StatusBadge
          label="Yük 1dk"
          value={load1 != null ? load1.toFixed(2) : "—"}
          accent={load1 != null ? loadAccent(load1, vps.cpu_count) : "text-text-secondary"}
        />
        <StatusBadge
          label="Yük 5/15dk"
          value={
            load5 != null && load15 != null
              ? `${load5.toFixed(2)} / ${load15.toFixed(2)}`
              : "—"
          }
          accent="text-text-secondary"
        />
        <StatusBadge
          label="Bellek"
          value={
            vps.memory
              ? `%${vps.memory.used_pct} · ${formatKb(vps.memory.used_kb)}/${formatKb(vps.memory.total_kb)}`
              : "—"
          }
          accent={vps.memory ? pctAccent(vps.memory.used_pct) : "text-text-secondary"}
        />
        <StatusBadge
          label={`Disk ${vps.disk?.mount ?? ""}`.trim()}
          value={
            vps.disk
              ? `%${vps.disk.used_pct} · ${formatKb(vps.disk.used_kb)}/${formatKb(vps.disk.total_kb)}`
              : "—"
          }
          accent={vps.disk ? pctAccent(vps.disk.used_pct) : "text-text-secondary"}
        />
      </div>
    </div>
  );
}

function StatusBadge({
  label,
  value,
  accent,
}: {
  label: string;
  value: string;
  accent: string;
}) {
  return (
    <div className="rounded border border-surface-700 bg-surface-800 px-3 py-2">
      <div className="font-mono text-[10px] uppercase tracking-wider text-text-muted truncate">
        {label}
      </div>
      <div className={`font-mono text-[13px] font-medium mt-0.5 ${accent} truncate`}>
        {value}
      </div>
    </div>
  );
}
