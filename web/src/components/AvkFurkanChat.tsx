/**
 * Furkan → AVK ajan chat widget — FUR-4164.
 *
 * Hedef ajan dropdown + textarea + Gönder. `POST /api/avk/furkan-chat`
 * `memory_signal_send` wrapper'ı çağırır — broadcast'tan farklı olarak
 * mesaj tmux pane'e değil ajan inbox'ına düşer (idle iken bekler, ajan
 * loop turn'ünde `memory_signal_read` ile yakalar).
 *
 * Son thread_id localStorage'da tutulur (same agent ile süregelen sohbet).
 */

import { useEffect, useState } from "react";
import { fetchAvkAgents, postAvkFurkanChat } from "../lib/api";
import type { AvkAgentInfo } from "../lib/types";

const THREAD_KEY_PREFIX = "avk-furkan-chat-thread:";

function loadThreadId(slug: string): string | undefined {
  if (typeof window === "undefined") return undefined;
  try {
    return window.localStorage.getItem(THREAD_KEY_PREFIX + slug) ?? undefined;
  } catch {
    return undefined;
  }
}

function saveThreadId(slug: string, threadId: string) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(THREAD_KEY_PREFIX + slug, threadId);
  } catch {
    // quota / disabled — sessizce geç
  }
}

type SendResult =
  | { kind: "idle" }
  | { kind: "sent"; signalId: string; threadId: string; at: string }
  | { kind: "error"; message: string };

export function AvkFurkanChat() {
  const [agents, setAgents] = useState<AvkAgentInfo[]>([]);
  const [selectedSlug, setSelectedSlug] = useState<string>("koord");
  const [message, setMessage] = useState("");
  const [sending, setSending] = useState(false);
  const [result, setResult] = useState<SendResult>({ kind: "idle" });
  const [useThread, setUseThread] = useState(true);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      const list = await fetchAvkAgents();
      if (!cancelled && list.length > 0) {
        setAgents(list);
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, []);

  const canSend = message.trim().length > 0 && !sending && selectedSlug.length > 0;
  const threadHint = loadThreadId(selectedSlug);

  async function handleSend() {
    if (!canSend) return;
    setSending(true);
    setResult({ kind: "idle" });
    const payload = {
      to: selectedSlug,
      message: message.trim(),
      thread_id: useThread ? threadHint : undefined,
    };
    const res = await postAvkFurkanChat(payload);
    setSending(false);
    if (!res) {
      setResult({ kind: "error", message: "Gönderim başarısız (404/502/ağ)." });
      return;
    }
    saveThreadId(selectedSlug, res.thread_id);
    setResult({
      kind: "sent",
      signalId: res.signal_id,
      threadId: res.thread_id,
      at: res.created_at,
    });
    setMessage("");
  }

  function handleNewThread() {
    if (typeof window === "undefined") return;
    try {
      window.localStorage.removeItem(THREAD_KEY_PREFIX + selectedSlug);
    } catch {
      // sessiz
    }
    setResult({ kind: "idle" });
  }

  return (
    <div>
      <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-3">
        Furkan → Ajan
        <span className="ml-2 normal-case tracking-normal text-text-dim text-[11px]">
          · agentmemory signal inbox
        </span>
      </h3>

      <div className="rounded border border-surface-700 bg-surface-800 p-4 space-y-3">
        <div className="flex items-end gap-3 flex-wrap">
          <div className="flex-1 min-w-[160px]">
            <label
              htmlFor="avk-furkan-chat-slug"
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
                id="avk-furkan-chat-slug"
                value={selectedSlug}
                onChange={(e) => {
                  setSelectedSlug(e.target.value);
                  setResult({ kind: "idle" });
                }}
                disabled={sending}
                className="w-full rounded border border-surface-700 bg-surface-900 px-3 py-2 font-body text-[14px] text-text-primary focus:outline-none focus:border-brand-500/60 focus:ring-1 focus:ring-brand-500/40"
              >
                {agents.map((agent) => (
                  <option key={agent.slug} value={agent.slug}>
                    {agent.label} · {agent.slug}
                  </option>
                ))}
              </select>
            )}
          </div>
          <label className="flex items-center gap-2 font-mono text-[11px] text-text-muted cursor-pointer">
            <input
              type="checkbox"
              checked={useThread}
              onChange={(e) => setUseThread(e.target.checked)}
              disabled={sending}
              className="accent-brand-500"
            />
            sohbeti devam ettir
          </label>
          {threadHint && useThread && (
            <button
              type="button"
              onClick={handleNewThread}
              disabled={sending}
              className="font-mono text-[10px] text-text-muted hover:text-status-error transition-colors"
              title="Mevcut thread'i unut, sonraki mesaj yeni thread aç"
            >
              yeni thread
            </button>
          )}
        </div>

        <div>
          <label
            htmlFor="avk-furkan-chat-message"
            className="font-mono text-[11px] uppercase tracking-wider text-text-muted block mb-1"
          >
            Mesaj
          </label>
          <textarea
            id="avk-furkan-chat-message"
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            onKeyDown={(e) => {
              if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
                e.preventDefault();
                if (canSend) void handleSend();
              }
            }}
            disabled={sending}
            rows={3}
            placeholder="Ör: koord, son patrol özet ver"
            className="w-full rounded border border-surface-700 bg-surface-900 px-3 py-2 font-body text-[14px] text-text-primary placeholder:text-text-muted/60 focus:outline-none focus:border-brand-500/60 focus:ring-1 focus:ring-brand-500/40 resize-y"
            maxLength={8192}
          />
          <div className="font-mono text-[10px] text-text-muted mt-1">
            {message.length}/8192 · gönder için{" "}
            <kbd className="px-1 py-0.5 rounded bg-surface-700 border border-surface-600 text-[9px]">
              ⌘↵
            </kbd>{" "}
            /{" "}
            <kbd className="px-1 py-0.5 rounded bg-surface-700 border border-surface-600 text-[9px]">
              Ctrl↵
            </kbd>
          </div>
        </div>

        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={handleSend}
            disabled={!canSend}
            className="rounded bg-brand-500 hover:bg-brand-400 disabled:bg-surface-700 disabled:text-text-muted disabled:cursor-not-allowed text-surface-900 font-mono text-sm font-semibold px-4 py-2 transition-colors"
          >
            {sending ? "Gönderiliyor…" : `Gönder → ${selectedSlug || "?"}`}
          </button>
          {result.kind === "error" && (
            <span className="font-body text-[13px] text-status-error">
              {result.message}
            </span>
          )}
          {result.kind === "sent" && (
            <span className="font-body text-[13px] text-status-running">
              ✓ signal {result.signalId.slice(0, 16)}… · thread{" "}
              {result.threadId.slice(0, 12)}…
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
