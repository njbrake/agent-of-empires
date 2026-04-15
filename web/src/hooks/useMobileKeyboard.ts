import { useEffect, useState } from "react";

// Detects touch-primary devices and tracks soft-keyboard state via visualViewport.
// isMobile is used to decide whether the mobile toolbar renders at all.
// keyboardOpen and keyboardHeight are used for POSITIONING the toolbar above
// the soft keyboard, not for gating whether it renders.
export function useMobileKeyboard() {
  const [isMobile, setIsMobile] = useState(() =>
    typeof window !== "undefined" &&
    window.matchMedia?.("(pointer: coarse)").matches,
  );
  const [keyboardOpen, setKeyboardOpen] = useState(false);
  const [keyboardHeight, setKeyboardHeight] = useState(0);

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

    const handleResize = () => {
      const heightDiff = window.innerHeight - vv.height;
      // Threshold avoids false positives from browser chrome changes.
      const open = heightDiff > 150;
      setKeyboardOpen(open);
      setKeyboardHeight(open ? heightDiff : 0);
      window.dispatchEvent(new Event("resize"));
    };
    handleResize();
    vv.addEventListener("resize", handleResize);
    return () => vv.removeEventListener("resize", handleResize);
  }, [isMobile]);

  return { isMobile, keyboardOpen, keyboardHeight };
}
