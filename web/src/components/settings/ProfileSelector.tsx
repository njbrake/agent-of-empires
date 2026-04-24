import { useCallback, useEffect, useState } from "react";
import {
  createProfile,
  deleteProfile,
  fetchProfiles,
  renameProfile,
  setDefaultProfile,
} from "../../lib/api";
import type { ProfileInfo } from "../../lib/types";

interface Props {
  selectedProfile: string | null;
  onSelect: (profile: string | null) => void;
}

export function ProfileSelector({ selectedProfile, onSelect }: Props) {
  const [profiles, setProfiles] = useState<ProfileInfo[]>([]);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    fetchProfiles().then(setProfiles);
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const activeProfile = profiles.find((p) => p.is_default);

  const handleCreate = async () => {
    const trimmed = newName.trim();
    if (!trimmed) return;
    if (!/^[a-zA-Z0-9_-]+$/.test(trimmed)) {
      setError("Only letters, digits, hyphens, and underscores");
      return;
    }
    const ok = await createProfile(trimmed);
    if (ok) {
      setCreating(false);
      setNewName("");
      setError(null);
      load();
    } else {
      setError("Failed to create profile");
    }
  };

  const handleDelete = async (name: string) => {
    if (!confirm(`Delete profile "${name}"?`)) return;
    const ok = await deleteProfile(name);
    if (ok) {
      if (selectedProfile === name) onSelect(null);
      load();
    }
  };

  const handleRename = async (name: string) => {
    const newN = prompt("New name:", name);
    if (!newN || newN === name) return;
    const ok = await renameProfile(name, newN);
    if (ok) {
      if (selectedProfile === name) onSelect(newN);
      load();
    }
  };

  const handleSetDefault = async (name: string) => {
    const ok = await setDefaultProfile(name);
    if (ok) load();
  };

  return (
    <div className="border border-surface-700/40 rounded-lg px-4 py-3 bg-surface-850">
      <div className="flex items-center gap-3">
        <label className="text-sm text-text-dim shrink-0">Profile</label>
        <select
          value={selectedProfile ?? ""}
          onChange={(e) => onSelect(e.target.value || null)}
          className="flex-1 bg-surface-900 border border-surface-700 rounded-md px-2 py-1.5 text-sm text-text-primary focus:border-brand-600 focus:outline-none"
        >
          <option value="">Global (all profiles)</option>
          {profiles.map((p) => (
            <option key={p.name} value={p.name}>
              {p.name}
              {p.is_default ? " (active)" : ""}
            </option>
          ))}
        </select>
        <button
          onClick={() => setCreating(!creating)}
          className="text-xs text-brand-500 hover:text-brand-400 cursor-pointer shrink-0"
        >
          + New
        </button>
      </div>

      {creating && (
        <div className="mt-2 flex gap-2">
          <input
            type="text"
            value={newName}
            onChange={(e) => {
              setNewName(e.target.value);
              setError(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleCreate();
              if (e.key === "Escape") {
                setCreating(false);
                setError(null);
              }
            }}
            placeholder="Profile name"
            autoFocus
            className={`flex-1 bg-surface-900 border rounded-md px-2 py-1.5 text-sm text-text-primary focus:outline-none ${error ? "border-red-500" : "border-surface-700 focus:border-brand-600"}`}
          />
          <button
            onClick={handleCreate}
            className="px-3 py-1.5 rounded-md bg-brand-600 hover:bg-brand-500 text-sm font-medium text-surface-950 cursor-pointer"
          >
            Create
          </button>
        </div>
      )}
      {error && <div className="text-xs text-red-400 mt-1">{error}</div>}

      {selectedProfile && (
        <div className="mt-2 flex gap-2 text-xs">
          <button
            onClick={() => handleRename(selectedProfile)}
            className="text-text-dim hover:text-text-primary cursor-pointer"
          >
            Rename
          </button>
          {!activeProfile || activeProfile.name !== selectedProfile ? (
            <>
              <span className="text-surface-700">|</span>
              <button
                onClick={() => handleSetDefault(selectedProfile)}
                className="text-text-dim hover:text-text-primary cursor-pointer"
              >
                Set as default
              </button>
              <span className="text-surface-700">|</span>
              <button
                onClick={() => handleDelete(selectedProfile)}
                className="text-text-dim hover:text-red-400 cursor-pointer"
              >
                Delete
              </button>
            </>
          ) : null}
        </div>
      )}
    </div>
  );
}
