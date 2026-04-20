import { useCallback, useEffect, useState } from "react";
import type { DeleteSessionOptions } from "../lib/api";
import type { CleanupDefaults } from "../lib/types";

interface Props {
  sessionTitle: string;
  branchName: string | null;
  hasManagedWorktree: boolean;
  isSandboxed: boolean;
  cleanupDefaults: CleanupDefaults;
  onConfirm: (options: DeleteSessionOptions) => Promise<void>;
  onCancel: () => void;
}

export function DeleteSessionDialog({
  sessionTitle,
  branchName,
  hasManagedWorktree,
  isSandboxed,
  cleanupDefaults,
  onConfirm,
  onCancel,
}: Props) {
  const [deleteWorktree, setDeleteWorktree] = useState(hasManagedWorktree && cleanupDefaults.delete_worktree);
  const [forceDelete, setForceDelete] = useState(false);
  const [deleteBranch, setDeleteBranch] = useState(hasManagedWorktree && cleanupDefaults.delete_branch);
  const [deleteSandbox, setDeleteSandbox] = useState(isSandboxed && cleanupDefaults.delete_sandbox);
  const [deleting, setDeleting] = useState(false);

  const hasOptions = hasManagedWorktree || isSandboxed;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const handleConfirm = useCallback(async () => {
    setDeleting(true);
    try {
      await onConfirm({
        delete_worktree: deleteWorktree,
        delete_branch: deleteBranch,
        delete_sandbox: deleteSandbox,
        force_delete: forceDelete,
      });
    } catch {
      setDeleting(false);
    }
  }, [onConfirm, deleteWorktree, deleteBranch, deleteSandbox, forceDelete]);

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 bg-black/60 flex items-center justify-center z-50 animate-fade-in"
      onClick={onCancel}
    >
      <div
        className="bg-surface-800 border border-surface-700/50 rounded-lg w-[420px] max-w-[90vw] shadow-2xl animate-slide-up"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="px-5 py-4 border-b border-surface-700">
          <h2 className="text-sm font-semibold text-status-error">
            Delete Session
          </h2>
        </div>

        {/* Body */}
        <div className="px-5 py-4 space-y-3">
          <p className="text-[13px] text-text-secondary">
            Delete{" "}
            <span className="font-mono text-text-primary">{sessionTitle}</span>?
          </p>

          {hasOptions && (
            <div className="space-y-2 pt-1">
              {hasManagedWorktree && (
                <>
                  <Checkbox
                    checked={deleteWorktree}
                    onChange={setDeleteWorktree}
                    label="Delete worktree"
                    detail={branchName ? `Removes worktree for branch "${branchName}"` : undefined}
                  />
                  {deleteWorktree && (
                    <div className="pl-6">
                      <Checkbox
                        checked={forceDelete}
                        onChange={setForceDelete}
                        label="Force delete"
                        detail="Delete even if worktree has uncommitted changes"
                      />
                    </div>
                  )}
                  <Checkbox
                    checked={deleteBranch}
                    onChange={setDeleteBranch}
                    label="Delete branch"
                    detail={branchName ? `Removes branch "${branchName}"` : undefined}
                  />
                </>
              )}
              {isSandboxed && (
                <Checkbox
                  checked={deleteSandbox}
                  onChange={setDeleteSandbox}
                  label="Delete container"
                  detail="Removes the Docker sandbox container"
                />
              )}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex justify-end gap-3 px-5 py-3 border-t border-surface-700">
          <button
            onClick={onCancel}
            disabled={deleting}
            className="px-3 py-1.5 text-sm text-text-secondary hover:text-text-primary rounded-md hover:bg-surface-700/50 cursor-pointer transition-colors disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            onClick={handleConfirm}
            disabled={deleting}
            className="px-3 py-1.5 text-sm text-white bg-status-error/90 hover:bg-status-error rounded-md cursor-pointer transition-colors disabled:opacity-50 flex items-center gap-2"
          >
            {deleting && (
              <svg className="animate-spin h-3.5 w-3.5" viewBox="0 0 24 24">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
              </svg>
            )}
            {deleting ? "Deleting..." : "Delete"}
          </button>
        </div>
      </div>
    </div>
  );
}

function Checkbox({
  checked,
  onChange,
  label,
  detail,
}: {
  checked: boolean;
  onChange: (val: boolean) => void;
  label: string;
  detail?: string;
}) {
  return (
    <label className="flex items-start gap-2.5 cursor-pointer group">
      <span
        onClick={() => onChange(!checked)}
        className={`mt-0.5 w-4 h-4 rounded border flex items-center justify-center shrink-0 transition-colors ${
          checked
            ? "bg-status-error border-status-error"
            : "border-surface-600 group-hover:border-surface-500"
        }`}
      >
        {checked && (
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
            <path d="M2 5L4 7L8 3" stroke="white" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        )}
      </span>
      <span className="flex flex-col min-w-0">
        <span className="text-[13px] text-text-secondary group-hover:text-text-primary transition-colors">
          {label}
        </span>
        {detail && (
          <span className="text-[12px] text-text-dim">{detail}</span>
        )}
      </span>
    </label>
  );
}
