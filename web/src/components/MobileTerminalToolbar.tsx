import { useCallback, useEffect, useState } from "react";
import type { Terminal } from "@xterm/xterm";
import type { RefObject } from "react";
import { useLongPressDrag, type DragAxis } from "../hooks/useLongPressDrag";

interface Props {
  sendData: (data: string) => void;
  termRef: RefObject<Terminal | null>;
  keyboardHeight: number;
}

const ARROW_UP = "\x1b[A";
const ARROW_DOWN = "\x1b[B";
const ARROW_LEFT = "\x1b[D";
const ARROW_RIGHT = "\x1b[C";

export function MobileTerminalToolbar({
  sendData,
  termRef,
  keyboardHeight,
}: Props) {
  const [ctrlActive, setCtrlActive] = useState(false);
  const [upAxis, setUpAxis] = useState<DragAxis>("vertical");
  const [downAxis, setDownAxis] = useState<DragAxis>("vertical");

  const haptic = useCallback(() => {
    navigator.vibrate?.(10);
  }, []);

  // Reset ctrl flag once the terminal consumes a keystroke after it was armed.
  useEffect(() => {
    const term = termRef.current;
    if (!term || !ctrlActive) return;
    const disposable = term.onData(() => setCtrlActive(false));
    return () => disposable.dispose();
  }, [ctrlActive, termRef]);

  const refocusTerminal = useCallback(() => {
    termRef.current?.focus();
  }, [termRef]);

  const send = useCallback(
    (data: string) => {
      haptic();
      sendData(data);
      refocusTerminal();
    },
    [sendData, refocusTerminal, haptic],
  );

  const upHandlers = useLongPressDrag({
    onRepeat: () => sendData(ARROW_UP),
    onHorizontal: (dir) => sendData(dir === "left" ? ARROW_LEFT : ARROW_RIGHT),
    onAxisChange: setUpAxis,
  });
  const downHandlers = useLongPressDrag({
    onRepeat: () => sendData(ARROW_DOWN),
    onHorizontal: (dir) => sendData(dir === "left" ? ARROW_LEFT : ARROW_RIGHT),
    onAxisChange: setDownAxis,
  });

  const btnBase =
    "flex-1 flex items-center justify-center h-11 rounded-md transition-colors duration-75 text-text-secondary select-none touch-manipulation relative active:bg-surface-700/50 active:scale-95";

  const strip =
    "shrink-0 flex items-center gap-1 px-2 py-1.5 bg-surface-850 border-t border-surface-700/20 safe-area-bottom";

  // Parent (TerminalView) reserves paddingBottom for the keyboard, so the
  // strip naturally sits above it. env(keyboard-inset-height) covers iPadOS
  // floating keyboards where visualViewport doesn't shrink.
  const stripStyle = {
    paddingBottom: keyboardHeight > 0 ? undefined : "env(keyboard-inset-height, 0px)",
  };

  const arrowHint = (axis: DragAxis) =>
    axis !== "vertical" ? (
      <span
        aria-hidden="true"
        className="absolute bottom-0.5 left-1/2 -translate-x-1/2 font-mono text-[9px] text-brand-400"
      >
        ←→
      </span>
    ) : null;

  return (
    <div
      className={strip}
      style={stripStyle}
      // Prevent toolbar taps from stealing focus away from the proxy input.
      // Without this, every button tap blurs the proxy and iOS closes the
      // soft keyboard. onClick handlers still fire normally.
      onMouseDown={(e) => e.preventDefault()}
    >
      <button type="button" aria-label="Arrow up" className={btnBase} {...upHandlers}>
        <span className="font-mono text-sm">{"\u2191"}</span>
        {arrowHint(upAxis)}
      </button>
      <button type="button" aria-label="Arrow down" className={btnBase} {...downHandlers}>
        <span className="font-mono text-sm">{"\u2193"}</span>
        {arrowHint(downAxis)}
      </button>
      <button type="button" aria-label="Tab" className={btnBase}
        onClick={() => send("\t")}>
        <span className="font-mono text-sm">Tab</span>
      </button>
      <button type="button" aria-label="Escape" className={btnBase}
        onClick={() => send("\x1b")}>
        <span className="font-mono text-sm">Esc</span>
      </button>
      <button
        type="button"
        aria-label="Ctrl"
        aria-pressed={ctrlActive}
        className={
          ctrlActive
            ? `${btnBase.replace("text-text-secondary", "text-brand-400")} bg-brand-600/20`
            : btnBase
        }
        onClick={() => { haptic(); setCtrlActive((v) => !v); refocusTerminal(); }}
      >
        <span className="font-mono text-xs">Ctrl</span>
      </button>
      <button type="button" aria-label="Ctrl+C interrupt" className={btnBase}
        onClick={() => { send("\x03"); setCtrlActive(false); }}>
        <span className="font-mono text-xs">^C</span>
      </button>
    </div>
  );
}
