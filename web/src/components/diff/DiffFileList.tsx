import { useCallback, useMemo, useRef, useState } from "react";
import type { RichDiffFile } from "../../lib/types";
import { buildDiffTree, type DiffTreeNode } from "../../lib/diffTree";
import { useWebSettings } from "../../hooks/useWebSettings";

interface Props {
  files: RichDiffFile[];
  baseBranch: string;
  warning: string | null;
  selectedPath: string | null;
  loading: boolean;
  onSelectFile: (path: string) => void;
}

const STATUS_COLORS: Record<string, string> = {
  added: "text-status-running",
  modified: "text-status-waiting",
  deleted: "text-status-error",
  renamed: "text-accent-600",
  copied: "text-accent-600",
  untracked: "text-text-muted",
  conflicted: "text-status-waiting",
};

const STATUS_LETTERS: Record<string, string> = {
  added: "A",
  modified: "M",
  deleted: "D",
  renamed: "R",
  copied: "C",
  untracked: "?",
  conflicted: "U",
};

// Unique key for a tree node (dir path or file path)
function nodeKey(node: DiffTreeNode): string {
  return node.kind === "dir" ? `dir:${node.path}` : node.file.path;
}

function FlatList({
  files,
  selectedPath,
  onSelectFile,
  focusedIndex,
  onFocusIndex,
}: {
  files: RichDiffFile[];
  selectedPath: string | null;
  onSelectFile: (path: string) => void;
  focusedIndex: number;
  onFocusIndex: (i: number) => void;
}) {
  return (
    <>
      {files.map((file, i) => {
        const parts = file.path.split("/");
        const fileName = parts.pop() || file.path;
        const dirPath = parts.length > 0 ? parts.join("/") + "/" : "";
        const isSelected = file.path === selectedPath;
        const isFocused = i === focusedIndex;

        return (
          <button
            key={file.path}
            data-index={i}
            onClick={() => onSelectFile(file.path)}
            onMouseEnter={() => onFocusIndex(i)}
            className={`w-full text-left px-3 py-1.5 cursor-pointer transition-colors flex items-center gap-2 ${
              isSelected
                ? "bg-surface-850 text-text-primary"
                : "text-text-secondary hover:bg-surface-800/50"
            } ${isFocused ? "outline outline-1 outline-brand-600/60 -outline-offset-1" : ""}`}
          >
            <span
              className={`shrink-0 font-mono text-[12px] w-3 text-center ${STATUS_COLORS[file.status] ?? "text-text-muted"}`}
            >
              {STATUS_LETTERS[file.status] ?? "?"}
            </span>
            <span className="truncate min-w-0 flex-1">
              {dirPath && (
                <span className="font-mono text-[11px] text-text-dim">
                  {dirPath}
                </span>
              )}
              <span className="font-mono text-[12px]">{fileName}</span>
            </span>
            <span className="shrink-0 font-mono text-[11px] flex items-center gap-1">
              {file.additions > 0 && (
                <span className="text-status-running">+{file.additions}</span>
              )}
              {file.deletions > 0 && (
                <span className="text-status-error">-{file.deletions}</span>
              )}
            </span>
          </button>
        );
      })}
    </>
  );
}

function TreeView({
  nodes,
  selectedPath,
  onSelectFile,
  onToggleDir,
  focusedIndex,
  onFocusIndex,
}: {
  nodes: DiffTreeNode[];
  selectedPath: string | null;
  onSelectFile: (path: string) => void;
  onToggleDir: (dirPath: string) => void;
  focusedIndex: number;
  onFocusIndex: (i: number) => void;
}) {
  return (
    <>
      {nodes.map((node, i) => {
        const isFocused = i === focusedIndex;
        const focusRing = isFocused
          ? "outline outline-1 outline-brand-600/60 -outline-offset-1"
          : "";

        if (node.kind === "dir") {
          return (
            <button
              key={nodeKey(node)}
              data-index={i}
              onClick={() => onToggleDir(node.path)}
              onMouseEnter={() => onFocusIndex(i)}
              className={`w-full text-left py-1.5 cursor-pointer transition-colors flex items-center gap-1.5 text-text-muted hover:bg-surface-800/50 ${focusRing}`}
              style={{ paddingLeft: `${node.depth * 16 + 12}px`, paddingRight: 12 }}
            >
              <svg
                className={`w-3 h-3 shrink-0 text-text-dim transition-transform duration-75 ${
                  node.collapsed ? "-rotate-90" : ""
                }`}
                viewBox="0 0 16 16"
                fill="currentColor"
              >
                <path d="M4 6l4 4 4-4" />
              </svg>
              <span className="font-mono text-[12px] truncate flex-1">
                {node.name}
              </span>
              <span className="shrink-0 font-mono text-[10px] text-text-dim">
                {node.fileCount}
              </span>
              <span className="shrink-0 font-mono text-[11px] flex items-center gap-1">
                {node.additions > 0 && (
                  <span className="text-status-running">+{node.additions}</span>
                )}
                {node.deletions > 0 && (
                  <span className="text-status-error">-{node.deletions}</span>
                )}
              </span>
            </button>
          );
        }

        const file = node.file;
        const fileName = file.path.split("/").pop() || file.path;
        const isSelected = file.path === selectedPath;

        return (
          <button
            key={file.path}
            data-index={i}
            onClick={() => onSelectFile(file.path)}
            onMouseEnter={() => onFocusIndex(i)}
            className={`w-full text-left py-1.5 cursor-pointer transition-colors flex items-center gap-2 ${
              isSelected
                ? "bg-surface-850 text-text-primary"
                : "text-text-secondary hover:bg-surface-800/50"
            } ${focusRing}`}
            style={{ paddingLeft: `${node.depth * 16 + 12}px`, paddingRight: 12 }}
          >
            <span
              className={`shrink-0 font-mono text-[12px] w-3 text-center ${STATUS_COLORS[file.status] ?? "text-text-muted"}`}
            >
              {STATUS_LETTERS[file.status] ?? "?"}
            </span>
            <span className="font-mono text-[12px] truncate flex-1">
              {fileName}
            </span>
            <span className="shrink-0 font-mono text-[11px] flex items-center gap-1">
              {file.additions > 0 && (
                <span className="text-status-running">+{file.additions}</span>
              )}
              {file.deletions > 0 && (
                <span className="text-status-error">-{file.deletions}</span>
              )}
            </span>
          </button>
        );
      })}
    </>
  );
}

export function DiffFileList({
  files,
  baseBranch,
  warning,
  selectedPath,
  loading,
  onSelectFile,
}: Props) {
  const { settings, update } = useWebSettings();
  const viewMode = settings.diffViewMode;
  const [collapsedDirs, setCollapsedDirs] = useState<Set<string>>(new Set());
  const [focusedIndex, setFocusedIndex] = useState(-1);
  const listRef = useRef<HTMLDivElement>(null);

  const totalAdditions = files.reduce((sum, f) => sum + f.additions, 0);
  const totalDeletions = files.reduce((sum, f) => sum + f.deletions, 0);

  const treeNodes = useMemo(
    () => buildDiffTree(files, collapsedDirs),
    [files, collapsedDirs],
  );

  const toggleDir = useCallback((dirPath: string) => {
    setCollapsedDirs((prev) => {
      const next = new Set(prev);
      if (next.has(dirPath)) {
        next.delete(dirPath);
      } else {
        next.add(dirPath);
      }
      return next;
    });
  }, []);

  const toggleViewMode = useCallback(() => {
    const next = viewMode === "flat" ? "tree" : "flat";
    update({ diffViewMode: next });
    setFocusedIndex(-1);
  }, [viewMode, update]);

  // Total number of visible items for keyboard nav
  const itemCount = viewMode === "tree" ? treeNodes.length : files.length;

  // Clamp focused index when item count shrinks
  const clampedFocusedIndex =
    focusedIndex >= itemCount ? -1 : focusedIndex;

  // Keyboard navigation
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (itemCount === 0) return;

      switch (e.key) {
        case "ArrowDown": {
          e.preventDefault();
          setFocusedIndex((prev) => {
            const next = prev < itemCount - 1 ? prev + 1 : prev;
            // Scroll into view
            listRef.current
              ?.querySelector(`[data-index="${next}"]`)
              ?.scrollIntoView({ block: "nearest" });
            return next;
          });
          break;
        }
        case "ArrowUp": {
          e.preventDefault();
          setFocusedIndex((prev) => {
            const next = prev > 0 ? prev - 1 : 0;
            listRef.current
              ?.querySelector(`[data-index="${next}"]`)
              ?.scrollIntoView({ block: "nearest" });
            return next;
          });
          break;
        }
        case "ArrowRight": {
          const rNode = viewMode === "tree" ? treeNodes[clampedFocusedIndex] : undefined;
          if (rNode?.kind === "dir" && rNode.collapsed) {
            e.preventDefault();
            toggleDir(rNode.path);
          }
          break;
        }
        case "ArrowLeft": {
          const lNode = viewMode === "tree" ? treeNodes[clampedFocusedIndex] : undefined;
          if (lNode?.kind === "dir" && !lNode.collapsed) {
            e.preventDefault();
            toggleDir(lNode.path);
          }
          break;
        }
        case "Enter":
        case " ": {
          e.preventDefault();
          if (clampedFocusedIndex < 0) break;
          if (viewMode === "tree") {
            const eNode = treeNodes[clampedFocusedIndex];
            if (eNode?.kind === "dir") {
              toggleDir(eNode.path);
            } else if (eNode?.kind === "file") {
              onSelectFile(eNode.file.path);
            }
          } else {
            const eFile = files[clampedFocusedIndex];
            if (eFile) onSelectFile(eFile.path);
          }
          break;
        }
      }
    },
    [itemCount, clampedFocusedIndex, viewMode, treeNodes, files, toggleDir, onSelectFile],
  );

  return (
    <div className="flex flex-col h-full bg-surface-900 overflow-hidden">
      {/* Header */}
      <div className="px-3 py-2 border-b border-surface-700/20 shrink-0">
        <div className="flex items-center gap-2 flex-wrap">
          <span className="font-mono text-[11px] uppercase tracking-wider text-text-dim">
            Changes
          </span>
          <span className="font-mono text-[10px] px-1.5 py-px rounded bg-surface-800 text-text-muted">
            vs {baseBranch}
          </span>
          {files.length > 0 && (
            <>
              <span className="font-mono text-[11px] text-text-muted">
                {files.length} file{files.length !== 1 ? "s" : ""}
              </span>
              <span className="font-mono text-[11px]">
                <span className="text-status-running">+{totalAdditions}</span>
                {" "}
                <span className="text-status-error">-{totalDeletions}</span>
              </span>
            </>
          )}
          {/* View mode toggle */}
          {files.length > 0 && (
            <button
              onClick={toggleViewMode}
              className="ml-auto shrink-0 p-1 rounded text-text-dim hover:text-text-muted hover:bg-surface-800/50 transition-colors cursor-pointer"
              title={viewMode === "flat" ? "Switch to tree view" : "Switch to flat list"}
            >
              {viewMode === "flat" ? (
                // Tree icon
                <svg
                  className="w-3.5 h-3.5"
                  viewBox="0 0 16 16"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.5"
                >
                  <path d="M2 3h12M5 7h9M5 11h9M2 7l1.5 1L2 9M2 11l1.5 1L2 13" />
                </svg>
              ) : (
                // List icon
                <svg
                  className="w-3.5 h-3.5"
                  viewBox="0 0 16 16"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.5"
                >
                  <path d="M2 3h12M2 7h12M2 11h12" />
                </svg>
              )}
            </button>
          )}
        </div>
        {warning && (
          <p className="text-[11px] text-status-waiting mt-1">{warning}</p>
        )}
      </div>

      {/* File list */}
      <div
        ref={listRef}
        className="flex-1 overflow-y-auto"
        tabIndex={0}
        onKeyDown={handleKeyDown}
      >
        {loading && files.length === 0 ? (
          <div className="flex items-center justify-center h-full text-text-dim">
            <span className="text-xs">Loading files...</span>
          </div>
        ) : files.length === 0 ? (
          <div className="flex items-center justify-center h-full text-text-dim">
            <div className="text-center px-4">
              <div className="font-mono text-xl text-surface-700 mb-1">0</div>
              <p className="text-xs">No changes yet</p>
            </div>
          </div>
        ) : viewMode === "tree" ? (
          <TreeView
            nodes={treeNodes}
            selectedPath={selectedPath}
            onSelectFile={onSelectFile}
            onToggleDir={toggleDir}
            focusedIndex={clampedFocusedIndex}
            onFocusIndex={setFocusedIndex}
          />
        ) : (
          <FlatList
            files={files}
            selectedPath={selectedPath}
            onSelectFile={onSelectFile}
            focusedIndex={clampedFocusedIndex}
            onFocusIndex={setFocusedIndex}
          />
        )}
      </div>
    </div>
  );
}
