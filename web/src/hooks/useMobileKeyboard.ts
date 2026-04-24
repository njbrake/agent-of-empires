import { useEffect, useRef, useState } from "react";

// Detects touch-primary devices and tracks soft-keyboard state via visualViewport.
// isMobile is used to decide whether the mobile toolbar renders at all.
// keyboardHeight is the extra padding needed to keep content above the keyboard;
// it accounts for what the layout viewport already handled and subtracts the
// bottom safe-area inset (the App root pads for it).
export function useMobileKeyboard() {
  const [isMobile, setIsMobile] = useState(() =>
    typeof window !== "undefined" &&
    window.matchMedia?.("(pointer: coarse)").matches,
  );
  const [keyboardOpen, setKeyboardOpen] = useState(false);
  const [keyboardHeight, setKeyboardHeight] = useState(0);
  const rafRef = useRef(0);
  const stableCountRef = useRef(0);
  const lastOcclusionRef = useRef(0);
  // Track the max viewport height seen (before keyboard opens) so we can
  // detect keyboard-open even when innerHeight shrinks with the keyboard.
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

    // Read the bottom safe-area inset once. The App root applies this as
    // padding, so the keyboard compensation should not include it.
    const safeBottom = parseFloat(
      getComputedStyle(document.documentElement)
        .getPropertyValue("--safe-area-bottom"),
    ) || 0;

    const measure = () => {
      const currentVvH = vv.height;

      // Update the full height when viewport grows (keyboard closed,
      // orientation change, etc.).
      if (currentVvH > fullHeightRef.current - 50) {
        fullHeightRef.current = Math.max(fullHeightRef.current, currentVvH);
      }

      // Detect keyboard open: significant drop from remembered full height.
      const totalOcclusion = fullHeightRef.current - currentVvH;
      const open = totalOcclusion > 100;

      // The padding we need is just the gap between innerHeight and the
      // visual viewport, minus the bottom safe area the App root already
      // handles. When innerHeight shrinks with the keyboard (iOS PWA,
      // iOS 26 Safari), innerHeight ≈ vvHeight and padding ≈ 0 (the flex
      // layout already accounted for it).
      const padding = open
        ? Math.max(0, window.innerHeight - currentVvH - safeBottom)
        : 0;

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
    // fire every frame during it. Poll via rAF to catch the transition,
    // stopping early when the measurement stabilizes (same value 3 frames
    // in a row) or after 20 frames max to avoid burning CPU while typing.
    const MAX_POLL_FRAMES = 20;
    const STABLE_THRESHOLD = 3;
    const startPolling = () => {
      cancelAnimationFrame(rafRef.current);
      stableCountRef.current = 0;
      let frameCount = 0;
      const poll = () => {
        frameCount++;
        const occlusion = measure();
        if (Math.abs(occlusion - lastOcclusionRef.current) < 1) {
          stableCountRef.current++;
        } else {
          stableCountRef.current = 0;
        }
        lastOcclusionRef.current = occlusion;
        if (stableCountRef.current < STABLE_THRESHOLD && frameCount < MAX_POLL_FRAMES) {
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
    let orientTimer: ReturnType<typeof setTimeout> | null = null;
    const handleOrientationChange = () => {
      fullHeightRef.current = 0;
      if (orientTimer) clearTimeout(orientTimer);
      orientTimer = setTimeout(() => {
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
      if (orientTimer) clearTimeout(orientTimer);
      vv.removeEventListener("resize", handleViewportChange);
      vv.removeEventListener("scroll", handleViewportChange);
      document.removeEventListener("focusin", handleFocusIn);
      window.removeEventListener("orientationchange", handleOrientationChange);
    };
  }, [isMobile]);

  return { isMobile, keyboardOpen, keyboardHeight };
}
