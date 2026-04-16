import { useEffect, useRef, useState } from "react";

// Detects touch-primary devices and tracks soft-keyboard state via visualViewport.
// isMobile is used to decide whether the mobile toolbar renders at all.
// keyboardOpen tracks whether the keyboard is visible (for UI state like hiding
// the keyboard button). keyboardHeight is the portion of keyboard occlusion that
// the browser's layout viewport did NOT account for; apply as paddingBottom.
export function useMobileKeyboard() {
  const [isMobile, setIsMobile] = useState(() =>
    typeof window !== "undefined" &&
    window.matchMedia?.("(pointer: coarse)").matches,
  );
  const [keyboardOpen, setKeyboardOpen] = useState(false);
  const [keyboardHeight, setKeyboardHeight] = useState(0);
  const rafRef = useRef(0);
  const stableCountRef = useRef(0);
  // Track the max viewport height ever seen (before keyboard opens) so we can
  // detect keyboard-open on devices where innerHeight shrinks with the keyboard.
  const fullHeightRef = useRef(0);

  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) return;
    const mql = window.matchMedia("(pointer: coarse)");
    const onChange = () => setIsMobile(mql.matches);
    mql.addEventListener?.("change", onChange);
    return () => mql.removeEventListener?.("change", onChange);
  }, []);

  useEffect(() => {
    if (!isMobile) return;
    const vv = window.visualViewport;
    if (!vv) return;

    fullHeightRef.current = Math.max(window.innerHeight, vv.height);

    let lastOpen = false;
    let lastPadding = 0;

    const measure = () => {
      const currentVvH = vv.height;

      // Update the full height when viewport grows (keyboard closed,
      // orientation change, etc.).
      if (currentVvH > fullHeightRef.current - 50) {
        fullHeightRef.current = Math.max(fullHeightRef.current, currentVvH);
      }

      // Detect keyboard open: significant drop from remembered full height.
      const totalOcclusion = fullHeightRef.current - currentVvH - vv.offsetTop;
      const open = totalOcclusion > 100;

      // For paddingBottom: only pad for what the browser's layout viewport
      // did NOT handle. When innerHeight shrinks with the keyboard (iOS PWA,
      // iOS 26 Safari), the flex layout already accounts for most of the
      // keyboard, and paddingBottom would double-compensate.
      const layoutHandled = fullHeightRef.current - window.innerHeight;
      const extraOcclusion = Math.max(
        0,
        totalOcclusion - Math.max(0, layoutHandled),
      );
      const padding = open ? extraOcclusion : 0;

      if (open !== lastOpen || padding !== lastPadding) {
        lastOpen = open;
        lastPadding = padding;
        stableCountRef.current = 0;
        setKeyboardOpen(open);
        setKeyboardHeight(padding);
      }
      return totalOcclusion;
    };

    // iOS keyboard animation takes ~300ms but visualViewport events don't
    // fire every frame during it. Poll via rAF to catch the transition as
    // it happens, then stop once the value is stable for ~60 frames.
    const startPolling = () => {
      cancelAnimationFrame(rafRef.current);
      stableCountRef.current = 0;
      const poll = () => {
        measure();
        stableCountRef.current++;
        if (stableCountRef.current < 60) {
          rafRef.current = requestAnimationFrame(poll);
        }
      };
      rafRef.current = requestAnimationFrame(poll);
    };

    const handleViewportChange = () => {
      measure();
      startPolling();
    };

    // Also poll briefly when any focusin happens; keyboard may be about
    // to open but visualViewport hasn't started updating yet.
    const handleFocusIn = (e: FocusEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") {
        startPolling();
      }
    };

    // Orientation changes reset the full height baseline.
    const handleOrientationChange = () => {
      fullHeightRef.current = 0;
      setTimeout(() => {
        fullHeightRef.current = Math.max(window.innerHeight, vv.height);
        measure();
      }, 500);
    };

    measure();
    vv.addEventListener("resize", handleViewportChange);
    vv.addEventListener("scroll", handleViewportChange);
    document.addEventListener("focusin", handleFocusIn);
    window.addEventListener("orientationchange", handleOrientationChange);
    return () => {
      cancelAnimationFrame(rafRef.current);
      vv.removeEventListener("resize", handleViewportChange);
      vv.removeEventListener("scroll", handleViewportChange);
      document.removeEventListener("focusin", handleFocusIn);
      window.removeEventListener("orientationchange", handleOrientationChange);
    };
  }, [isMobile]);

  return { isMobile, keyboardOpen, keyboardHeight };
}
