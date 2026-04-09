import { useRef, useEffect } from "react";

interface Props {
  value: string;
  onChange: (value: string) => void;
  onClose: () => void;
}

export function SearchBar({ value, onChange, onClose }: Props) {
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      onClose();
    }
  };

  return (
    <div className="px-2 pb-2">
      <div className="flex items-center bg-surface-900 border border-surface-700 rounded px-2 py-1">
        <span className="font-mono text-[11px] text-slate-500 mr-1.5">/</span>
        <input
          ref={inputRef}
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Search sessions..."
          className="flex-1 bg-transparent font-body text-xs text-slate-200 placeholder:text-slate-600 focus:outline-none"
        />
        {value && (
          <button
            onClick={() => onChange("")}
            className="text-slate-500 hover:text-slate-300 text-xs cursor-pointer ml-1"
          >
            &times;
          </button>
        )}
      </div>
    </div>
  );
}
