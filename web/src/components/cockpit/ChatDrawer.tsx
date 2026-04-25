// Chat drawer. Mobile: bottom-anchored sheet that slides up. Desktop:
// right-side dock. Sends typed messages to the cockpit's prompt
// endpoint.

import { useCallback, useEffect, useState } from "react";

interface Props {
  sessionId: string;
  onSubmit: (text: string) => Promise<void>;
  /** Mobile: starts closed and slides up; desktop: always docked. */
  variant: "mobile" | "desktop";
}

export function ChatDrawer({ sessionId: _sessionId, onSubmit, variant }: Props) {
  const [open, setOpen] = useState(variant === "desktop");
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);

  // Keep desktop variant always open; mobile honors the toggle state.
  useEffect(() => {
    if (variant === "desktop") setOpen(true);
  }, [variant]);

  const submit = useCallback(async () => {
    if (!text.trim() || sending) return;
    setSending(true);
    try {
      await onSubmit(text);
      setText("");
    } finally {
      setSending(false);
    }
  }, [text, sending, onSubmit]);

  const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void submit();
    }
  };

  if (variant === "mobile" && !open) {
    return (
      <button
        type="button"
        className="fixed right-4 bottom-20 z-30 flex items-center gap-2 rounded-full bg-amber-600 px-5 py-3 text-white shadow-lg hover:bg-amber-500"
        aria-label="Open chat drawer"
        onClick={() => setOpen(true)}
      >
        💬 Chat
      </button>
    );
  }

  const wrapperClass =
    variant === "mobile"
      ? "fixed inset-x-0 bottom-0 z-30 max-h-[60vh] rounded-t-xl bg-slate-900 border-t border-slate-700 p-4 shadow-2xl"
      : "flex h-full flex-col bg-slate-900 border-l border-slate-700 p-3";

  return (
    <div className={wrapperClass}>
      {variant === "mobile" && (
        <div className="flex items-center justify-between mb-2">
          <span className="text-xs uppercase tracking-wide text-slate-400 font-mono">
            chat
          </span>
          <button
            type="button"
            className="text-slate-400 hover:text-slate-100 px-2 py-1 text-sm"
            onClick={() => setOpen(false)}
            aria-label="Close chat drawer"
          >
            close
          </button>
        </div>
      )}

      <textarea
        className="w-full rounded bg-slate-800 border border-slate-700 px-3 py-2 text-sm text-slate-100 focus:outline-none focus:ring-2 focus:ring-amber-600 resize-none"
        rows={variant === "mobile" ? 3 : 4}
        placeholder="Steer the agent…  (Enter to send, Shift+Enter for newline)"
        value={text}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={onKeyDown}
        disabled={sending}
      />
      <div className="flex justify-end mt-2">
        <button
          type="button"
          className={`rounded font-medium py-2 px-4 text-sm ${
            sending
              ? "bg-amber-700 text-slate-200 cursor-wait"
              : "bg-amber-600 text-white hover:bg-amber-500"
          }`}
          disabled={sending || !text.trim()}
          onClick={() => void submit()}
        >
          {sending ? "Sending…" : "Send"}
        </button>
      </div>
    </div>
  );
}
