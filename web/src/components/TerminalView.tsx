import { useCallback, useEffect, useRef } from "react";
import type { FormEvent } from "react";
import { useTerminal } from "../hooks/useTerminal";
import { useMobileKeyboard } from "../hooks/useMobileKeyboard";
import { useWebSettings } from "../hooks/useWebSettings";
import { MobileTerminalToolbar } from "./MobileTerminalToolbar";
import type { SessionResponse } from "../lib/types";
import "@xterm/xterm/css/xterm.css";

interface Props {
  session: SessionResponse;
}

export function TerminalView({ session }: Props) {
  const { containerRef, termRef, state, manualReconnect, sendData } =
    useTerminal(session.id);
  const { isMobile, keyboardHeight } = useMobileKeyboard();
  const { settings } = useWebSettings();
  const proxyRef = useRef<HTMLInputElement>(null);

  // Auto-open soft keyboard when a session is selected, if the user wants it.
  useEffect(() => {
    if (!isMobile || !state.connected) return;
    if (!settings.autoOpenKeyboard) return;
    const id = setTimeout(() => proxyRef.current?.focus(), 50);
    return () => clearTimeout(id);
  }, [isMobile, state.connected, session.id, settings.autoOpenKeyboard]);

  // The proxy input is the keyboard bridge: soft keyboard types into it,
  // we relay each input to the PTY and clear. Mobile browsers don't
  // reliably expose xterm's own helper textarea for the soft keyboard.
  const onProxyInput = useCallback(
    (e: FormEvent<HTMLInputElement>) => {
      const value = e.currentTarget.value;
      if (value) sendData(value);
      e.currentTarget.value = "";
    },
    [sendData],
  );

  // Tap the terminal pane to reopen the keyboard. Skip when text is
  // selected (preserves native long-press-to-select behavior).
  const onContainerClick = useCallback(() => {
    if (!isMobile) return;
    const selection = window.getSelection()?.toString() ?? "";
    if (selection.length > 0) return;
    proxyRef.current?.focus();
  }, [isMobile]);

  return (
    <div className="flex-1 flex flex-col overflow-hidden relative">
      {!state.connected && state.reconnecting && (
        <div className="bg-status-waiting/15 border-b border-status-waiting/30 px-4 py-1.5 flex items-center gap-2 shrink-0">
          <span className="text-xs text-status-waiting">
            Reconnecting in {state.retryCountdown}s... ({state.retryCount}/3)
          </span>
        </div>
      )}
      {!state.connected && !state.reconnecting && state.retryCount >= 3 && (
        <div className="bg-status-error/10 border-b border-status-error/30 px-4 py-1.5 flex items-center gap-2 shrink-0">
          <span className="text-xs text-status-error">
            Connection lost
          </span>
          <button
            onClick={manualReconnect}
            className="text-xs text-brand-500 hover:text-brand-400 cursor-pointer underline"
          >
            Retry
          </button>
        </div>
      )}

      <div className="flex-1 overflow-hidden bg-surface-950 relative">
        <div
          ref={containerRef}
          onClick={onContainerClick}
          className="absolute inset-0"
        />

        {isMobile && state.connected && (
          <input
            ref={proxyRef}
            type="text"
            autoComplete="off"
            autoCorrect="off"
            autoCapitalize="none"
            spellCheck={false}
            onInput={onProxyInput}
            aria-hidden="true"
            tabIndex={-1}
            className="absolute opacity-0 pointer-events-none w-px h-px -z-10"
            style={{ left: 0, top: 0 }}
          />
        )}
      </div>

      {isMobile && state.connected && (
        <MobileTerminalToolbar
          sendData={sendData}
          termRef={termRef}
          keyboardHeight={keyboardHeight}
        />
      )}
    </div>
  );
}
