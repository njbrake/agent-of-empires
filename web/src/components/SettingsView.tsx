import { useEffect, useState } from "react";
import { getSettings, updateSettings, fetchThemes } from "../lib/api";

interface Props {
  onClose: () => void;
}

type Category =
  | "theme"
  | "session"
  | "sandbox"
  | "worktree"
  | "tmux"
  | "updates"
  | "sound";

const CATEGORIES: { key: Category; label: string }[] = [
  { key: "theme", label: "Theme" },
  { key: "session", label: "Session" },
  { key: "sandbox", label: "Sandbox" },
  { key: "worktree", label: "Worktree" },
  { key: "tmux", label: "Tmux" },
  { key: "updates", label: "Updates" },
  { key: "sound", label: "Sound" },
];

export function SettingsView({ onClose }: Props) {
  const [settings, setSettings] = useState<Record<string, unknown> | null>(
    null,
  );
  const [themes, setThemes] = useState<string[]>([]);
  const [activeCategory, setActiveCategory] = useState<Category>("theme");
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);

  useEffect(() => {
    getSettings().then(setSettings);
    fetchThemes().then(setThemes);
  }, []);

  const update = (section: string, key: string, value: unknown) => {
    if (!settings) return;
    const sectionData =
      (settings[section] as Record<string, unknown>) || {};
    setSettings({
      ...settings,
      [section]: { ...sectionData, [key]: value },
    });
    setDirty(true);
  };

  const handleSave = async () => {
    if (!settings) return;
    setSaving(true);
    await updateSettings(settings);
    setSaving(false);
    setDirty(false);
  };

  const get = (section: string, key: string): unknown => {
    if (!settings) return undefined;
    const s = settings[section] as Record<string, unknown> | undefined;
    return s?.[key];
  };

  if (!settings) {
    return (
      <div className="flex-1 flex items-center justify-center bg-surface-900 text-text-muted font-mono text-sm">
        Loading settings...
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-surface-900">
      {/* Header */}
      <div className="h-12 bg-surface-850 border-b border-surface-700 flex items-center px-4 shrink-0">
        <button
          onClick={onClose}
          className="text-brand-500 mr-3 cursor-pointer font-body text-sm"
        >
          &larr; Back
        </button>
        <span className="font-display text-sm font-semibold text-text-bright">
          Settings
        </span>
        <div className="ml-auto flex items-center gap-2">
          {dirty && (
            <span className="font-mono text-sm text-status-warning">
              unsaved
            </span>
          )}
          <button
            onClick={handleSave}
            disabled={!dirty || saving}
            className="px-3 py-1 font-body text-xs rounded-md bg-brand-600 text-white hover:bg-brand-700 disabled:opacity-40 cursor-pointer transition-colors"
          >
            {saving ? "Saving..." : "Save"}
          </button>
        </div>
      </div>

      <div className="flex flex-1 overflow-hidden">
        {/* Category sidebar */}
        <div className="w-[180px] border-r border-surface-700 py-2">
          {CATEGORIES.map((cat) => (
            <button
              key={cat.key}
              onClick={() => setActiveCategory(cat.key)}
              className={`w-full text-left px-4 py-1.5 font-body text-xs cursor-pointer transition-colors ${
                activeCategory === cat.key
                  ? "text-brand-500 bg-brand-600/10 border-l-2 border-brand-600 pl-3.5"
                  : "text-text-secondary hover:text-text-primary hover:bg-surface-800"
              }`}
            >
              {cat.label}
            </button>
          ))}
        </div>

        {/* Settings form */}
        <div className="flex-1 overflow-y-auto p-6 max-w-[600px]">
          {activeCategory === "theme" && (
            <Section title="Theme">
              <SelectField
                label="Theme"
                value={(get("theme", "name") as string) || "empire"}
                options={themes}
                onChange={(v) => update("theme", "name", v)}
              />
            </Section>
          )}

          {activeCategory === "session" && (
            <Section title="Session Defaults">
              <TextField
                label="Default Tool"
                value={(get("session", "default_tool") as string) || ""}
                onChange={(v) => update("session", "default_tool", v)}
                placeholder="claude"
              />
              <TextField
                label="Extra Args"
                value={(get("session", "extra_args") as string) || ""}
                onChange={(v) => update("session", "extra_args", v)}
                placeholder="--resume ..."
              />
              <BoolField
                label="Agent Status Hooks"
                value={(get("session", "agent_status_hooks") as boolean) ?? true}
                onChange={(v) => update("session", "agent_status_hooks", v)}
              />
            </Section>
          )}

          {activeCategory === "sandbox" && (
            <Section title="Sandbox">
              <BoolField
                label="Sandbox by Default"
                value={
                  (get("sandbox", "sandbox_by_default") as boolean) ?? false
                }
                onChange={(v) => update("sandbox", "sandbox_by_default", v)}
              />
              <TextField
                label="Default Image"
                value={(get("sandbox", "default_image") as string) || ""}
                onChange={(v) => update("sandbox", "default_image", v)}
                placeholder="ubuntu:latest"
              />
              <TextField
                label="CPU Limit"
                value={(get("sandbox", "cpu_limit") as string) || ""}
                onChange={(v) => update("sandbox", "cpu_limit", v)}
                placeholder="2"
              />
              <TextField
                label="Memory Limit"
                value={(get("sandbox", "memory_limit") as string) || ""}
                onChange={(v) => update("sandbox", "memory_limit", v)}
                placeholder="4G"
              />
              <BoolField
                label="Mount SSH"
                value={(get("sandbox", "mount_ssh") as boolean) ?? true}
                onChange={(v) => update("sandbox", "mount_ssh", v)}
              />
            </Section>
          )}

          {activeCategory === "worktree" && (
            <Section title="Worktree">
              <TextField
                label="Path Template"
                value={(get("worktree", "path_template") as string) || ""}
                onChange={(v) => update("worktree", "path_template", v)}
                placeholder="../{branch}"
              />
              <BoolField
                label="Auto Cleanup"
                value={(get("worktree", "auto_cleanup") as boolean) ?? false}
                onChange={(v) => update("worktree", "auto_cleanup", v)}
              />
              <BoolField
                label="Delete Branch on Cleanup"
                value={
                  (get("worktree", "delete_branch_on_cleanup") as boolean) ??
                  false
                }
                onChange={(v) =>
                  update("worktree", "delete_branch_on_cleanup", v)
                }
              />
            </Section>
          )}

          {activeCategory === "tmux" && (
            <Section title="Tmux">
              <SelectField
                label="Status Bar"
                value={(get("tmux", "status_bar") as string) || "bottom"}
                options={["top", "bottom", "off"]}
                onChange={(v) => update("tmux", "status_bar", v)}
              />
              <BoolField
                label="Mouse Mode"
                value={(get("tmux", "mouse_mode") as boolean) ?? true}
                onChange={(v) => update("tmux", "mouse_mode", v)}
              />
            </Section>
          )}

          {activeCategory === "updates" && (
            <Section title="Updates">
              <BoolField
                label="Check for Updates"
                value={(get("updates", "check_enabled") as boolean) ?? true}
                onChange={(v) => update("updates", "check_enabled", v)}
              />
              <TextField
                label="Check Interval (hours)"
                value={String((get("updates", "check_interval_hours") as number) || 24)}
                onChange={(v) =>
                  update("updates", "check_interval_hours", parseInt(v) || 24)
                }
              />
            </Section>
          )}

          {activeCategory === "sound" && (
            <Section title="Sound">
              <BoolField
                label="Sound Enabled"
                value={(get("sound", "enabled") as boolean) ?? false}
                onChange={(v) => update("sound", "enabled", v)}
              />
            </Section>
          )}
        </div>
      </div>
    </div>
  );
}

// --- Reusable field components ---

function Section({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-4">
        {title}
      </h3>
      <div className="space-y-4">{children}</div>
    </div>
  );
}

function TextField({
  label,
  value,
  onChange,
  placeholder,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
}) {
  return (
    <label className="block">
      <span className="font-body text-xs text-text-secondary block mb-1">
        {label}
      </span>
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="w-full bg-surface-900 border border-surface-700 rounded px-3 py-1.5 font-body text-sm text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
      />
    </label>
  );
}

function BoolField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex items-center justify-between py-1 cursor-pointer">
      <span className="font-body text-xs text-text-secondary">{label}</span>
      <button
        type="button"
        onClick={() => onChange(!value)}
        className={`w-9 h-5 rounded-full transition-colors relative cursor-pointer ${
          value ? "bg-brand-600" : "bg-surface-700"
        }`}
      >
        <span
          className={`absolute top-0.5 w-4 h-4 rounded-full bg-white shadow transition-transform ${
            value ? "translate-x-4" : "translate-x-0.5"
          }`}
        />
      </button>
    </label>
  );
}

function SelectField({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: string[];
  onChange: (v: string) => void;
}) {
  return (
    <label className="block">
      <span className="font-body text-xs text-text-secondary block mb-1">
        {label}
      </span>
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="w-full bg-surface-900 border border-surface-700 rounded px-3 py-1.5 font-body text-sm text-text-primary focus:border-brand-600 focus:outline-none"
      >
        {options.map((opt) => (
          <option key={opt} value={opt}>
            {opt}
          </option>
        ))}
      </select>
    </label>
  );
}
