import { useCallback, useEffect, useRef, useState } from "react";

const SPLIT_STORAGE_KEY = "aoe-split-ratio";
const DEFAULT_DIFF_WIDTH = 380;
const MIN_TERMINAL_WIDTH = 400;
const MIN_DIFF_WIDTH = 280;

interface Props {
  left: React.ReactNode;
  right: React.ReactNode;
  collapsed: boolean;
  onToggleCollapse: () => void;
}

function loadSavedWidth(): number {
  try {
    const saved = localStorage.getItem(SPLIT_STORAGE_KEY);
    if (saved) {
      const w = parseInt(saved, 10);
      if (w >= MIN_DIFF_WIDTH) return w;
    }
  } catch {
    // ignore
  }
  return DEFAULT_DIFF_WIDTH;
}

export function ContentSplit({
  left,
  right,
  collapsed,
  onToggleCollapse,
}: Props) {
  const [diffWidth, setDiffWidth] = useState(loadSavedWidth);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    dragging.current = true;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }, []);

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!dragging.current || !containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      const newDiffWidth = rect.right - e.clientX;
      const terminalWidth = rect.width - newDiffWidth;

      if (
        newDiffWidth >= MIN_DIFF_WIDTH &&
        terminalWidth >= MIN_TERMINAL_WIDTH
      ) {
        setDiffWidth(newDiffWidth);
      }
    };

    const handleMouseUp = () => {
      if (!dragging.current) return;
      dragging.current = false;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      // Persist
      setDiffWidth((w) => {
        localStorage.setItem(SPLIT_STORAGE_KEY, String(w));
        return w;
      });
      // Trigger resize for xterm fit
      window.dispatchEvent(new Event("resize"));
    };

    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);
    return () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
    };
  }, []);

  // Re-fit terminal when collapsed state changes
  useEffect(() => {
    window.dispatchEvent(new Event("resize"));
  }, [collapsed]);

  return (
    <div ref={containerRef} className="flex-1 flex min-h-0 overflow-hidden relative">
      {/* Terminal pane */}
      <div className="flex-1 flex flex-col min-w-0 min-h-0">{left}</div>

      {!collapsed && (
        <>
          {/* Drag handle (desktop) */}
          <div
            onMouseDown={handleMouseDown}
            onDoubleClick={onToggleCollapse}
            className="hidden md:block w-1 cursor-col-resize shrink-0 hover:bg-brand-600/50 transition-colors duration-75"
          />

          {/* Right pane: inline on desktop, overlay on mobile */}
          <div
            style={{ width: diffWidth }}
            className="hidden md:flex shrink-0 flex-col min-h-0 overflow-hidden"
          >
            {right}
          </div>

          {/* Mobile: slide-in panel from right with backdrop (mirrors left sidebar pattern) */}
          <div
            className="md:hidden fixed inset-0 bg-black/50 z-30"
            onClick={onToggleCollapse}
          />
          <div className="md:hidden fixed inset-y-0 right-0 z-40 w-[85vw] max-w-sm flex flex-col bg-surface-900">
            <div className="h-10 flex items-center px-3 border-b border-surface-700/20 shrink-0">
              <span className="text-sm text-text-muted flex-1">
                Diff & Shell
              </span>
              <button
                onClick={onToggleCollapse}
                className="w-8 h-8 flex items-center justify-center text-text-dim hover:text-text-secondary hover:bg-surface-800 cursor-pointer rounded-md transition-colors"
              >
                &times;
              </button>
            </div>
            <div className="flex-1 flex flex-col min-h-0 overflow-hidden">
              {right}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
