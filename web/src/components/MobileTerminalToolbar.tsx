import { useCallback, useEffect, useRef, useState } from "react";
import type { Terminal } from "@xterm/xterm";
import type { RefObject } from "react";

interface Props {
  sendData: (data: string) => void;
  termRef: RefObject<Terminal | null>;
}

interface KeyDef {
  label: string;
  ariaLabel: string;
  data: string;
  repeatable?: boolean;
}

const KEYS: KeyDef[] = [
  { label: "\u2190", ariaLabel: "Arrow left", data: "\x1b[D", repeatable: true },
  { label: "\u2191", ariaLabel: "Arrow up", data: "\x1b[A", repeatable: true },
  { label: "\u2193", ariaLabel: "Arrow down", data: "\x1b[B", repeatable: true },
  { label: "\u2192", ariaLabel: "Arrow right", data: "\x1b[C", repeatable: true },
  { label: "Tab", ariaLabel: "Tab", data: "\t" },
  { label: "Esc", ariaLabel: "Escape", data: "\x1b" },
];

const LONG_PRESS_DELAY = 300;
const LONG_PRESS_INTERVAL = 100;

export function MobileTerminalToolbar({ sendData, termRef }: Props) {
  const [ctrlActive, setCtrlActive] = useState(false);
  const repeatTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const repeatInterval = useRef<ReturnType<typeof setInterval> | null>(null);

  // Clean up timers on unmount
  useEffect(() => {
    return () => {
      if (repeatTimer.current) clearTimeout(repeatTimer.current);
      if (repeatInterval.current) clearInterval(repeatInterval.current);
    };
  }, []);

  // Listen for terminal data events to apply Ctrl modifier
  useEffect(() => {
    const term = termRef.current;
    if (!term || !ctrlActive) return;

    const disposable = term.onData(() => {
      setCtrlActive(false);
    });

    return () => disposable.dispose();
  }, [ctrlActive, termRef]);

  const haptic = useCallback(() => {
    navigator.vibrate?.(10);
  }, []);

  const handleSend = useCallback(
    (data: string) => {
      haptic();
      sendData(data);
      // Refocus terminal to keep soft keyboard open
      termRef.current?.focus();
    },
    [sendData, termRef, haptic],
  );

  const handleKeyPress = useCallback(
    (key: KeyDef) => {
      handleSend(key.data);
    },
    [handleSend],
  );

  const handleCtrlToggle = useCallback(() => {
    haptic();
    setCtrlActive((prev) => !prev);
    termRef.current?.focus();
  }, [termRef, haptic]);

  const handleCtrlC = useCallback(() => {
    handleSend("\x03");
    setCtrlActive(false);
  }, [handleSend]);

  const clearRepeat = useCallback(() => {
    if (repeatTimer.current) {
      clearTimeout(repeatTimer.current);
      repeatTimer.current = null;
    }
    if (repeatInterval.current) {
      clearInterval(repeatInterval.current);
      repeatInterval.current = null;
    }
  }, []);

  const handlePointerDown = useCallback(
    (key: KeyDef) => {
      if (!key.repeatable) return;
      clearRepeat();
      repeatTimer.current = setTimeout(() => {
        repeatInterval.current = setInterval(() => {
          handleSend(key.data);
        }, LONG_PRESS_INTERVAL);
      }, LONG_PRESS_DELAY);
    },
    [handleSend, clearRepeat],
  );

  const handlePointerUp = useCallback(() => {
    clearRepeat();
  }, [clearRepeat]);

  const btnBase =
    "flex-1 flex items-center justify-center h-11 rounded-md transition-colors duration-75 text-text-secondary select-none touch-manipulation";
  const btnDefault = `${btnBase} active:bg-surface-700/50 active:scale-95`;
  const btnCtrl = ctrlActive
    ? `${btnBase} bg-brand-600/20 text-brand-400`
    : btnDefault;

  return (
    <div className="shrink-0 flex items-center gap-1 px-2 py-1.5 bg-surface-850 border-t border-surface-700/20 safe-area-bottom">
      {KEYS.map((key) => (
        <button
          key={key.ariaLabel}
          type="button"
          aria-label={key.ariaLabel}
          className={btnDefault}
          onClick={() => handleKeyPress(key)}
          onPointerDown={() => handlePointerDown(key)}
          onPointerUp={handlePointerUp}
          onPointerCancel={handlePointerUp}
          onPointerLeave={handlePointerUp}
        >
          <span className="font-mono text-sm">{key.label}</span>
        </button>
      ))}

      <button
        type="button"
        aria-label="Ctrl"
        className={btnCtrl}
        onClick={handleCtrlToggle}
      >
        <span className="font-mono text-xs">Ctrl</span>
      </button>

      <button
        type="button"
        aria-label="Ctrl+C interrupt"
        className={btnDefault}
        onClick={handleCtrlC}
      >
        <span className="font-mono text-xs">^C</span>
      </button>
    </div>
  );
}
