interface Props {
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  title,
  message,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  danger = false,
  onConfirm,
  onCancel,
}: Props) {
  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-surface-800 border border-surface-700 rounded-md w-dialog max-w-[90vw] shadow-xl">
        <div className="px-5 pt-4 pb-3">
          <h3 className="font-body text-sm font-semibold text-text-primary">
            {title}
          </h3>
          <p className="font-body text-sm text-text-secondary mt-2">{message}</p>
        </div>
        <div className="flex justify-end gap-2 px-5 py-3 border-t border-surface-700">
          <button
            onClick={onCancel}
            className="px-3 py-1.5 font-body text-xs rounded-md text-text-secondary hover:bg-surface-700 transition-colors cursor-pointer"
          >
            {cancelLabel}
          </button>
          <button
            onClick={onConfirm}
            className={`px-3 py-1.5 font-body text-xs rounded-md transition-colors cursor-pointer ${
              danger
                ? "bg-status-error/20 text-status-error border border-status-error/30 hover:bg-status-error/30"
                : "bg-brand-600/20 text-brand-500 border border-brand-600/30 hover:bg-brand-600/30"
            }`}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
