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
    <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50 animate-fade-in">
      <div className="bg-surface-800 border border-surface-700/50 rounded-xl w-dialog max-w-[90vw] shadow-2xl animate-slide-up">
        <div className="px-6 pt-5 pb-4">
          <h3 className="font-display text-base font-semibold text-text-primary">
            {title}
          </h3>
          <p className="font-body text-sm text-text-secondary mt-2 leading-relaxed">
            {message}
          </p>
        </div>
        <div className="flex justify-end gap-2 px-6 py-4 border-t border-surface-700/30">
          <button
            onClick={onCancel}
            className="px-4 py-2 font-body text-sm rounded-lg text-text-secondary hover:bg-surface-700/30 transition-colors cursor-pointer"
          >
            {cancelLabel}
          </button>
          <button
            onClick={onConfirm}
            className={`px-4 py-2 font-body text-sm font-medium rounded-lg transition-colors cursor-pointer ${
              danger
                ? "bg-status-error text-white hover:bg-red-600"
                : "bg-brand-600 text-white hover:bg-brand-700"
            }`}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
