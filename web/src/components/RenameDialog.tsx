import { useState } from "react";

interface Props {
  currentTitle: string;
  currentGroup: string;
  onSave: (title: string, group: string) => void;
  onCancel: () => void;
}

export function RenameDialog({
  currentTitle,
  currentGroup,
  onSave,
  onCancel,
}: Props) {
  const [title, setTitle] = useState(currentTitle);
  const [group, setGroup] = useState(currentGroup);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    onSave(title, group);
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <form
        onSubmit={handleSubmit}
        className="bg-surface-800 border border-surface-700 rounded-md w-dialog max-w-[90vw] shadow-xl"
      >
        <div className="px-5 pt-4 pb-3">
          <h3 className="font-body text-sm font-semibold text-text-primary mb-3">
            Rename Session
          </h3>
          <label className="block mb-3">
            <span className="font-mono text-label uppercase tracking-wider text-text-muted block mb-1">
              Title
            </span>
            <input
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              autoFocus
              className="w-full bg-surface-900 border border-surface-700 rounded px-3 py-1.5 font-body text-sm text-text-primary focus:border-brand-600 focus:outline-none"
            />
          </label>
          <label className="block">
            <span className="font-mono text-label uppercase tracking-wider text-text-muted block mb-1">
              Group
            </span>
            <input
              type="text"
              value={group}
              onChange={(e) => setGroup(e.target.value)}
              placeholder="e.g. work/projects"
              className="w-full bg-surface-900 border border-surface-700 rounded px-3 py-1.5 font-body text-sm text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
            />
          </label>
        </div>
        <div className="flex justify-end gap-2 px-5 py-3 border-t border-surface-700">
          <button
            type="button"
            onClick={onCancel}
            className="px-3 py-1.5 font-body text-xs rounded-md text-text-secondary hover:bg-surface-700 transition-colors cursor-pointer"
          >
            Cancel
          </button>
          <button
            type="submit"
            className="px-3 py-1.5 font-body text-xs rounded-md bg-brand-600/20 text-brand-500 border border-brand-600/30 hover:bg-brand-600/30 transition-colors cursor-pointer"
          >
            Save
          </button>
        </div>
      </form>
    </div>
  );
}
