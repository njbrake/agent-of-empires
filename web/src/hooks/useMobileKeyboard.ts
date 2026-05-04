import { useEffect, useRef, useState } from "react";

const RESERVATION_STORAGE_KEY = "aoe-mobile-keyboard-reservation";

// Initial value for reservedKeyboardHeight on mobile. Returning visitors
// see exactly the size they latched last time (no first-open resize); new
// visitors get a sensible default (~40% of innerHeight, clamped to 200) so
// the layout still starts at a keyboard-reserved size and the first real
// measurement either matches or causes one small adjustment.
function readReservationSeed(): number {
  if (typeof window === "undefined") return 0;
  if (!window.matchMedia?.("(pointer: coarse)").matches) return 0;
  try {
    const saved = localStorage.getItem(RESERVATION_STORAGE_KEY);
    if (saved) {
      const n = parseInt(saved, 10);
      if (Number.isFinite(n) && n > 0 && n < window.innerHeight) return n;
    }
  } catch {
    // ignore
  }
  return Math.max(200, Math.floor(window.innerHeight * 0.4));
}

// Detects touch-primary devices and tracks soft-keyboard state via visualViewport.
// isMobile is used to decide whether the mobile toolbar renders at all.
// keyboardHeight is the extra padding needed to keep content above the keyboard
// for iOS regular Safari (where the layout viewport doesn't shrink); it stays
// 0 on iOS PWA and iOS 26 Safari, where innerHeight shrinks with the keyboard
// and the flex layout would already account for it (if we let it).
//
// reservedKeyboardHeight latches the largest visualViewport occlusion seen
// since the last orientation change. It's positive on every platform that has
// a soft keyboard (regular Safari, PWA, Android Chrome), and is the value
// consumers should use to size the layout. Once a keyboard has opened once,
// the reservation persists when it dismisses, so the layout stays "keyboard
// reserved" and we stop SIGWINCH-ing claude on every show/hide.
//
// stableViewportHeight is the largest window.innerHeight seen since the last
// orientation change. On iOS PWA / iOS 26 Safari / Android Chrome, innerHeight
// shrinks when the keyboard opens and the App root's `100dvh` would shrink
// with it; the App root applies this as an explicit pixel height instead so
// the layout stays at the no-keyboard size. Reset on orientation change.
export function useMobileKeyboard() {
  const [isMobile, setIsMobile] = useState(() =>
    typeof window !== "undefined" &&
    window.matchMedia?.("(pointer: coarse)").matches,
  );
  const [keyboardOpen, setKeyboardOpen] = useState(false);
  const [keyboardHeight, setKeyboardHeight] = useState(0);
  const [reservedKeyboardHeight, setReservedKeyboardHeight] =
    useState(readReservationSeed);
  const [stableViewportHeight, setStableViewportHeight] = useState(0);
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

  // Persist the latched reservation so the next page load lands on the
  // same value without any first-open resize.
  useEffect(() => {
    if (!isMobile || reservedKeyboardHeight <= 0) return;
    try {
      localStorage.setItem(
        RESERVATION_STORAGE_KEY,
        String(reservedKeyboardHeight),
      );
    } catch {
      // ignore (private mode, storage quota, etc.)
    }
  }, [isMobile, reservedKeyboardHeight]);

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
      // Latch reservation upward based on totalOcclusion (not padding):
      // padding stays 0 on iOS PWA and iOS 26 Safari because innerHeight
      // shrinks with the keyboard, so latching off padding there leaves
      // reservedKeyboardHeight at 0 and the fix becomes a no-op.
      // totalOcclusion is the true keyboard size on every platform.
      if (open && totalOcclusion > 0) {
        setReservedKeyboardHeight((prev) =>
          totalOcclusion > prev ? totalOcclusion : prev,
        );
      }
      // Latch the max layout-viewport height. On iOS PWA the keyboard
      // shrinks innerHeight, so without this 100dvh would also shrink and
      // resize the terminal container. App.tsx pins the root to this value.
      // Take the larger of innerHeight and vv.height so a mount that
      // happens to find the keyboard already open (innerHeight reduced)
      // can still latch to vv.height if that's somehow larger; in
      // practice both match in the no-keyboard state and that's what we
      // capture on first measure.
      const heightCandidate = Math.max(window.innerHeight, currentVvH);
      setStableViewportHeight((prev) =>
        heightCandidate > prev ? heightCandidate : prev,
      );
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

    // Orientation changes reset the full height baseline AND the
    // reservation: the keyboard physically swaps shape between portrait
    // and landscape, so a stale reservation would either crowd the
    // terminal or leave dead space below it.
    let orientTimer: ReturnType<typeof setTimeout> | null = null;
    const handleOrientationChange = () => {
      fullHeightRef.current = 0;
      setReservedKeyboardHeight(0);
      setStableViewportHeight(0);
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

  return {
    isMobile,
    keyboardOpen,
    keyboardHeight,
    reservedKeyboardHeight,
    stableViewportHeight,
  };
}
