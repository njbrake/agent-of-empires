import { useCallback, useEffect, useState } from "react";
import type { Terminal } from "@xterm/xterm";
import type { RefObject } from "react";

/**
 * Detects mobile touch devices and manages keyboard lifecycle:
 * - Tracks whether the soft keyboard is open (via visualViewport height)
 * - Fires terminal refit when the soft keyboard opens/closes
 * - Provides focusTerminal() to programmatically open the keyboard
 */
export function useMobileKeyboard(
  termRef: RefObject<Terminal | null>,
) {
  const [isMobile, setIsMobile] = useState(() =>
    typeof window !== "undefined" &&
    window.innerWidth < 768 &&
    navigator.maxTouchPoints > 0,
  );
  const [keyboardOpen, setKeyboardOpen] = useState(false);

  // Re-evaluate on resize (e.g., device rotation or resizing browser)
  useEffect(() => {
    const check = () => {
      setIsMobile(window.innerWidth < 768 && navigator.maxTouchPoints > 0);
    };
    window.addEventListener("resize", check);
    return () => window.removeEventListener("resize", check);
  }, []);

  // Detect keyboard open/close via visualViewport and trigger terminal refit.
  // When the keyboard opens, visualViewport.height shrinks well below
  // window.innerHeight. A threshold of 150px avoids false positives from
  // browser chrome changes.
  useEffect(() => {
    if (!isMobile) return;
    const vv = window.visualViewport;
    if (!vv) return;

    const handleResize = () => {
      const heightDiff = window.innerHeight - vv.height;
      setKeyboardOpen(heightDiff > 150);
      window.dispatchEvent(new Event("resize"));
    };
    vv.addEventListener("resize", handleResize);
    return () => vv.removeEventListener("resize", handleResize);
  }, [isMobile]);

  // Programmatically focus the terminal to open the soft keyboard.
  // Uses requestAnimationFrame delay for iOS Safari compatibility.
  const focusTerminal = useCallback(() => {
    if (!isMobile) return;
    requestAnimationFrame(() => {
      termRef.current?.focus();
    });
  }, [isMobile, termRef]);

  return { isMobile, keyboardOpen, focusTerminal };
}
