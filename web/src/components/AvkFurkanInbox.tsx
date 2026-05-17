/**
 * Furkan İnbox widget — FUR-4170.
 *
 * Ajan→Furkan mesaj akışı. `GET /api/avk/furkan-inbox` 30s polling.
 * Yeni signal ID görüldüğünde Browser Notification API ile bildirim
 * (kullanıcı izin verdiyse) + tarayıcı sekme başlığında okunmamış sayısı
 * rozeti (örn "(3) Agent of Empires").
 *
 * Furkan canon 2026-05-17: "ajanlar bana sorup bir şey söylediklerinde
 * benim görebileceğim bir alan ya da bildirim sistemi"
 *
 * ## Okunmamış sayımı
 *
 * Server-side `memory_signal_read` çağırı delivered'i read'e geçirir.
 * UI tarafında "yeni" tanımı: son polling turundan bu yana ilk kez görülen
 * signal_id. `seenIds` set'i localStorage'da tutulur — sayfa yenilendi
 * dahi son durumda kalan signal'ler tekrar bildirim üretmez.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { fetchAvkFurkanInbox } from "../lib/api";
import { usePushSubscription } from "../hooks/usePushSubscription";
import type { FurkanInboxSignal } from "../lib/types";

const REFRESH_INTERVAL_MS = 30_000;
const SEEN_KEY = "avk-furkan-inbox-seen";
const TITLE_BASE = "Agent of Empires";

function loadSeenIds(): Set<string> {
  if (typeof window === "undefined") return new Set();
  try {
    const raw = window.localStorage.getItem(SEEN_KEY);
    if (!raw) return new Set();
    const arr = JSON.parse(raw);
    return new Set(Array.isArray(arr) ? arr : []);
  } catch {
    return new Set();
  }
}

function saveSeenIds(ids: Set<string>) {
  if (typeof window === "undefined") return;
  try {
    // Sadece son 200 ID — sınırsız büyüme önle
    const arr = Array.from(ids).slice(-200);
    window.localStorage.setItem(SEEN_KEY, JSON.stringify(arr));
  } catch {
    // sessiz
  }
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

const TYPE_LABEL: Record<string, string> = {
  chat: "Sohbet",
  request: "İstek",
  question: "Soru",
  handoff: "Devir",
  alert: "Uyarı",
  report: "Rapor",
};

const TYPE_CLASS: Record<string, string> = {
  alert: "bg-status-error/15 text-status-error border-status-error/30",
  question: "bg-status-waiting/15 text-status-waiting border-status-waiting/30",
  request: "bg-status-waiting/15 text-status-waiting border-status-waiting/30",
  handoff: "bg-brand-500/15 text-brand-500 border-brand-500/30",
  report: "bg-status-running/15 text-status-running border-status-running/30",
  chat: "bg-surface-700 text-text-secondary border-surface-600",
};

type Permission = "default" | "granted" | "denied" | "unsupported";

function detectPermission(): Permission {
  if (typeof window === "undefined" || typeof Notification === "undefined") {
    return "unsupported";
  }
  return Notification.permission;
}

export function AvkFurkanInbox() {
  const [signals, setSignals] = useState<FurkanInboxSignal[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [unreadCount, setUnreadCount] = useState(0);
  const [permission, setPermission] = useState<Permission>(() => detectPermission());
  const push = usePushSubscription();
  const seenIdsRef = useRef<Set<string>>(loadSeenIds());
  const firstLoadRef = useRef(true);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      const res = await fetchAvkFurkanInbox(50);
      if (cancelled) return;
      if (!res) {
        setError("inbox endpoint ulaşılamadı");
        setLoading(false);
        return;
      }
      setError(null);
      setLoading(false);

      const seen = seenIdsRef.current;
      const newOnes: FurkanInboxSignal[] = [];
      for (const sig of res.signals) {
        if (!seen.has(sig.id)) {
          newOnes.push(sig);
        }
      }

      if (!firstLoadRef.current && newOnes.length > 0) {
        triggerNotification(newOnes);
        setUnreadCount((c) => c + newOnes.length);
      }

      for (const sig of res.signals) {
        seen.add(sig.id);
      }
      saveSeenIds(seen);
      firstLoadRef.current = false;
      setSignals(res.signals);
    }

    load();
    const id = setInterval(load, REFRESH_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  useEffect(() => {
    if (typeof document === "undefined") return;
    document.title =
      unreadCount > 0 ? `(${unreadCount}) ${TITLE_BASE}` : TITLE_BASE;
  }, [unreadCount]);

  async function requestPermission() {
    if (typeof Notification === "undefined") return;
    try {
      const result = await Notification.requestPermission();
      setPermission(result);
    } catch {
      // sessiz
    }
  }

  function clearUnread() {
    setUnreadCount(0);
  }

  const sortedSignals = useMemo(
    () =>
      [...signals].sort((a, b) => {
        const aT = new Date(a.created_at).getTime();
        const bT = new Date(b.created_at).getTime();
        return bT - aT;
      }),
    [signals],
  );

  return (
    <div>
      <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-3 flex items-center gap-2 flex-wrap">
        Ajan → Furkan
        <span className="normal-case tracking-normal text-text-dim text-[11px]">
          · agentmemory inbox
        </span>
        {unreadCount > 0 && (
          <button
            type="button"
            onClick={clearUnread}
            className="normal-case tracking-normal font-mono text-[11px] px-1.5 py-0.5 rounded bg-status-error text-surface-950 font-semibold cursor-pointer"
            title="Okunmadı rozetini temizle"
          >
            {unreadCount} yeni
          </button>
        )}
        {/* Foreground bildirim (sayfa açıkken balon) — basit Notification API */}
        {permission === "default" && (
          <button
            type="button"
            onClick={requestPermission}
            className="normal-case tracking-normal font-mono text-[11px] text-brand-500 hover:underline"
            title="Sayfa açıkken bildirim balonu"
          >
            bildirimleri aç
          </button>
        )}
        {permission === "denied" && (
          <span className="normal-case tracking-normal text-text-dim text-[11px]">
            · bildirim engelli
          </span>
        )}

        {/* Arka plan/kilit ekran push (Web Push API + Service Worker) */}
        {(push.state.kind === "off" || push.state.kind === "loading") && (
          <button
            type="button"
            onClick={push.enable}
            disabled={push.state.kind === "loading"}
            className="normal-case tracking-normal font-mono text-[11px] text-brand-500 hover:underline disabled:opacity-60"
            title="Arka plan / kilit ekran push bildirim (Service Worker)"
          >
            {push.state.kind === "loading" ? "yükleniyor…" : "push aboneliği aç"}
          </button>
        )}
        {push.state.kind === "asking" && (
          <span className="normal-case tracking-normal text-status-waiting text-[11px]">
            · izin isteniyor…
          </span>
        )}
        {push.state.kind === "subscribing" && (
          <span className="normal-case tracking-normal text-status-waiting text-[11px]">
            · abone olunuyor…
          </span>
        )}
        {push.state.kind === "enabled" && (
          <>
            <span className="normal-case tracking-normal text-status-running text-[11px]">
              · push aktif
            </span>
            <button
              type="button"
              onClick={push.sendTest}
              className="normal-case tracking-normal font-mono text-[10px] text-text-muted hover:text-brand-500"
              title="Sunucudan test push gönder"
            >
              test
            </button>
            <button
              type="button"
              onClick={push.disable}
              className="normal-case tracking-normal font-mono text-[10px] text-text-muted hover:text-status-error"
              title="Push aboneliğini kapat"
            >
              kapat
            </button>
          </>
        )}
        {push.state.kind === "sending-test" && (
          <span className="normal-case tracking-normal text-status-waiting text-[11px]">
            · test gönderiliyor…
          </span>
        )}
        {push.state.kind === "disabling" && (
          <span className="normal-case tracking-normal text-status-waiting text-[11px]">
            · kapatılıyor…
          </span>
        )}
        {push.state.kind === "denied" && (
          <span className="normal-case tracking-normal text-text-dim text-[11px]">
            · push izni reddedildi (iPhone Safari Ayarlar → Bildirimler)
          </span>
        )}
        {push.state.kind === "unsupported" && (
          <span className="normal-case tracking-normal text-text-dim text-[11px]" title={`reason: ${push.state.reason}`}>
            {push.state.reason === "ios-not-standalone"
              ? "· PWA install gerek (Ana Ekrana Ekle)"
              : push.state.reason === "insecure-origin"
                ? "· HTTPS gerek (Tailscale)"
                : "· push API desteklenmiyor"}
          </span>
        )}
        {push.state.kind === "error" && (
          <span className="normal-case tracking-normal text-status-error text-[11px]" title={push.state.message}>
            · push hata
          </span>
        )}
        {permission === "granted" && push.state.kind === "off" && (
          <span className="normal-case tracking-normal text-text-dim text-[10px]" title="Foreground bildirim (sayfa açık) çalışıyor — arka plan push için 'push aboneliği aç'">
            ↑ kilit ekran için
          </span>
        )}
      </h3>

      {loading ? (
        <p className="font-body text-[13px] text-text-muted">Yükleniyor…</p>
      ) : error ? (
        <p className="font-body text-[13px] text-status-error">{error}</p>
      ) : sortedSignals.length === 0 ? (
        <p className="font-body text-[13px] text-text-dim">
          Henüz ajan mesajı yok. Ajan `memory_signal_send to=furkan` ile yazarsa
          burada görünür.
        </p>
      ) : (
        <ul className="space-y-2 max-h-[28rem] overflow-y-auto">
          {sortedSignals.map((sig) => (
            <SignalItem key={sig.id} signal={sig} />
          ))}
        </ul>
      )}
    </div>
  );
}

function SignalItem({ signal }: { signal: FurkanInboxSignal }) {
  const typeLabel = TYPE_LABEL[signal.type] ?? signal.type;
  const typeClass = TYPE_CLASS[signal.type] ?? TYPE_CLASS.chat;
  return (
    <li className="rounded border border-surface-700 bg-surface-800 p-3">
      <div className="flex items-start justify-between gap-2 mb-1.5">
        <div className="flex items-center gap-2 flex-wrap min-w-0">
          <span className="font-mono text-[12px] font-medium text-text-primary truncate">
            {signal.from}
          </span>
          <span
            className={`font-mono text-[10px] uppercase tracking-wider border px-1.5 py-0.5 rounded ${typeClass}`}
          >
            {typeLabel}
          </span>
        </div>
        <span
          className="font-mono text-[10px] text-text-muted shrink-0"
          title={signal.created_at}
        >
          {formatRelativeTime(signal.created_at)}
        </span>
      </div>
      <p className="font-body text-[13px] text-text-secondary leading-relaxed whitespace-pre-wrap break-words">
        {signal.content}
      </p>
      {signal.thread_id && (
        <div className="mt-1.5 font-mono text-[10px] text-text-dim">
          thread {signal.thread_id.slice(0, 16)}…
        </div>
      )}
    </li>
  );
}

function triggerNotification(newOnes: FurkanInboxSignal[]) {
  if (typeof window === "undefined" || typeof Notification === "undefined") return;
  if (Notification.permission !== "granted") return;
  // Çoklu mesajda tek toplu bildirim
  if (newOnes.length === 1) {
    const sig = newOnes[0];
    if (!sig) return;
    try {
      new Notification(`${sig.from} → Furkan`, {
        body: sig.content.slice(0, 200),
        tag: sig.id,
        icon: "/icons/icon-192.png",
      });
    } catch {
      // sessiz
    }
    return;
  }
  try {
    const senders = Array.from(new Set(newOnes.map((s) => s.from))).join(", ");
    new Notification(`${newOnes.length} yeni ajan mesajı`, {
      body: `Gönderen: ${senders}`,
      tag: "avk-inbox-batch",
      icon: "/icons/icon-192.png",
    });
  } catch {
    // sessiz
  }
}
