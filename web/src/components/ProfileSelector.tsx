import { useEffect, useState, useRef } from "react";
import { fetchProfiles, createProfile, deleteProfile } from "../lib/api";

interface Props {
  activeProfile: string | null;
  onSelect: (profile: string | null) => void;
}

export function ProfileSelector({ activeProfile, onSelect }: Props) {
  const [profiles, setProfiles] = useState<string[]>([]);
  const [open, setOpen] = useState(false);
  const [newName, setNewName] = useState("");
  const [showCreate, setShowCreate] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    fetchProfiles().then(setProfiles);
  }, []);

  // Close on click outside
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
        setShowCreate(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, []);

  const handleCreate = async () => {
    if (!newName.trim()) return;
    const ok = await createProfile(newName.trim());
    if (ok) {
      setProfiles((p) => [...p, newName.trim()]);
      onSelect(newName.trim());
      setNewName("");
      setShowCreate(false);
      setOpen(false);
    }
  };

  const handleDelete = async (name: string) => {
    if (name === "default") return;
    const ok = await deleteProfile(name);
    if (ok) {
      setProfiles((p) => p.filter((n) => n !== name));
      if (activeProfile === name) onSelect(null);
    }
  };

  const display = activeProfile || "all profiles";

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen(!open)}
        className="font-mono text-[11px] text-slate-400 hover:text-slate-200 cursor-pointer px-2 py-1 rounded hover:bg-surface-800 transition-colors"
      >
        [{display}]
      </button>

      {open && (
        <div className="absolute top-full right-0 mt-1 w-48 bg-surface-800 border border-surface-700 rounded-md shadow-xl z-50">
          <button
            onClick={() => {
              onSelect(null);
              setOpen(false);
            }}
            className={`w-full text-left px-3 py-1.5 font-body text-xs cursor-pointer transition-colors ${
              !activeProfile
                ? "text-brand-500 bg-brand-600/10"
                : "text-slate-300 hover:bg-surface-700"
            }`}
          >
            All profiles
          </button>
          {profiles.map((p) => (
            <div key={p} className="flex items-center group">
              <button
                onClick={() => {
                  onSelect(p);
                  setOpen(false);
                }}
                className={`flex-1 text-left px-3 py-1.5 font-body text-xs cursor-pointer transition-colors ${
                  activeProfile === p
                    ? "text-brand-500 bg-brand-600/10"
                    : "text-slate-300 hover:bg-surface-700"
                }`}
              >
                {p}
              </button>
              {p !== "default" && (
                <button
                  onClick={() => handleDelete(p)}
                  className="px-2 py-1 text-[10px] text-status-error opacity-0 group-hover:opacity-100 cursor-pointer"
                  title="Delete profile"
                >
                  &times;
                </button>
              )}
            </div>
          ))}
          <div className="border-t border-surface-700">
            {showCreate ? (
              <div className="flex gap-1 p-2">
                <input
                  type="text"
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleCreate()}
                  autoFocus
                  placeholder="profile name"
                  className="flex-1 bg-surface-900 border border-surface-700 rounded px-2 py-1 font-body text-xs text-slate-200 placeholder:text-slate-600 focus:border-brand-600 focus:outline-none"
                />
                <button
                  onClick={handleCreate}
                  className="px-2 py-1 font-body text-[10px] text-brand-500 hover:bg-brand-600/10 rounded cursor-pointer"
                >
                  Add
                </button>
              </div>
            ) : (
              <button
                onClick={() => setShowCreate(true)}
                className="w-full text-left px-3 py-1.5 font-body text-xs text-slate-500 hover:text-slate-300 hover:bg-surface-700 cursor-pointer"
              >
                + New profile
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
