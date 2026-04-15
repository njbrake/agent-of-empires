import { useEffect, useRef, useState } from "react";
import { browseFilesystem } from "../../../lib/api";

interface WizardData {
  path: string;
  [key: string]: unknown;
}

interface Props {
  data: WizardData;
  onChange: (field: string, value: unknown) => void;
}

export function ProjectStep({ data, onChange }: Props) {
  const [pathSuggestions, setPathSuggestions] = useState<string[]>([]);
  const [showPathSuggestions, setShowPathSuggestions] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  useEffect(() => {
    if (!data.path) return;
    clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(async () => {
      const entries = await browseFilesystem(data.path);
      setPathSuggestions(entries.map((e) => e.path));
    }, 300);
    return () => clearTimeout(debounceRef.current);
  }, [data.path]);

  return (
    <div>
      <h2 className="text-lg font-semibold text-text-primary mb-1">Project folder</h2>
      <p className="text-sm text-text-muted mb-5">
        Enter the path to the project where the agent will work.
      </p>

      <div className="relative">
        <label className="block text-sm text-text-dim mb-1.5">Path</label>
        <input
          type="text"
          value={data.path}
          onChange={(e) => { onChange("path", e.target.value); setShowPathSuggestions(true); }}
          onBlur={() => setTimeout(() => setShowPathSuggestions(false), 200)}
          placeholder="/path/to/your/project"
          autoFocus
          className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-base font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
        />
        {showPathSuggestions && pathSuggestions.length > 0 && (
          <div className="absolute z-10 w-full mt-1 bg-surface-800 border border-surface-700 rounded-lg max-h-48 overflow-y-auto">
            {pathSuggestions.slice(0, 10).map((s) => (
              <button key={s} onMouseDown={() => { onChange("path", s); setShowPathSuggestions(false); }}
                className="w-full text-left px-3 py-2.5 text-sm font-mono text-text-secondary hover:bg-surface-700 cursor-pointer">{s}</button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
