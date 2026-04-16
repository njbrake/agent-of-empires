import { useCallback, useEffect, useState } from "react";
import { ConnectedDevices } from "./ConnectedDevices";
import { SecuritySettings } from "./SecuritySettings";
import { TerminalSettings } from "./TerminalSettings";
import { getSettings, updateSettings } from "../lib/api";

interface Props {
  onClose: () => void;
}

function CollapsibleSection({ title, badge, children, defaultOpen = false }: {
  title: string;
  badge?: string;
  children: React.ReactNode;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="border border-surface-700/40 rounded-lg overflow-hidden">
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center justify-between w-full px-4 py-3 bg-surface-850 hover:bg-surface-800 cursor-pointer transition-colors text-left"
      >
        <div className="flex items-center gap-2">
          <svg className={`w-3 h-3 text-text-dim transition-transform ${open ? "rotate-90" : ""}`} viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M4.5 2l4.5 4-4.5 4" />
          </svg>
          <span className="text-sm font-medium text-text-primary">{title}</span>
          {badge && <span className="text-[10px] font-mono text-text-dim bg-surface-700 px-1.5 py-0.5 rounded">{badge}</span>}
        </div>
      </button>
      {open && <div className="px-4 py-4 space-y-4 border-t border-surface-700/20">{children}</div>}
    </div>
  );
}

function ToggleField({ label, description, checked, onChange }: {
  label: string;
  description?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between gap-3">
      <div>
        <div className="text-sm text-text-primary">{label}</div>
        {description && <div className="text-xs text-text-dim mt-0.5">{description}</div>}
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={`relative inline-flex h-6 w-10 shrink-0 items-center rounded-full transition-colors cursor-pointer ${checked ? "bg-brand-600" : "bg-surface-700"}`}
      >
        <span className={`inline-block h-4 w-4 rounded-full bg-white shadow-sm transition-transform ${checked ? "translate-x-5" : "translate-x-1"}`} />
      </button>
    </div>
  );
}

function TextField({ label, value, onChange, placeholder, mono }: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  mono?: boolean;
}) {
  return (
    <div>
      <label className="block text-sm text-text-dim mb-1">{label}</label>
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className={`w-full bg-surface-900 border border-surface-700 rounded-md px-3 py-2 text-sm text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none ${mono ? "font-mono" : ""}`}
      />
    </div>
  );
}

export function SettingsView({ onClose }: Props) {
  const [settings, setSettings] = useState<Record<string, unknown> | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    getSettings().then((s) => { if (s) setSettings(s); });
  }, []);

  const save = useCallback(async (section: string, data: Record<string, unknown>) => {
    setSaving(true);
    await updateSettings({ [section]: data });
    setSaving(false);
  }, []);

  const session = (settings?.session ?? {}) as Record<string, unknown>;
  const sandbox = (settings?.sandbox ?? {}) as Record<string, unknown>;
  const worktree = (settings?.worktree ?? {}) as Record<string, unknown>;

  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-surface-900">
      <div className="h-12 bg-surface-850 border-b border-surface-700 flex items-center px-4 shrink-0">
        <button onClick={onClose} className="text-brand-500 mr-3 cursor-pointer text-sm">&larr; Back</button>
        <span className="text-sm font-semibold text-text-bright">Settings</span>
        {saving && <span className="ml-2 text-xs text-text-dim">Saving...</span>}
      </div>

      <div className="flex-1 overflow-y-auto p-6 max-w-[600px] space-y-4">
        <SecuritySettings />
        <TerminalSettings />

        {settings && (
          <>
            <CollapsibleSection title="Session Defaults">
              <TextField
                label="Default agent"
                value={(session.default_tool as string) ?? ""}
                onChange={(v) => {
                  setSettings({ ...settings, session: { ...session, default_tool: v || null } });
                  save("session", { ...session, default_tool: v || null });
                }}
                placeholder="Auto-detect"
                mono
              />
              <ToggleField
                label="YOLO mode by default"
                description="New sessions skip permission prompts"
                checked={(session.yolo_mode_default as boolean) ?? false}
                onChange={(v) => {
                  setSettings({ ...settings, session: { ...session, yolo_mode_default: v } });
                  save("session", { ...session, yolo_mode_default: v });
                }}
              />
            </CollapsibleSection>

            <CollapsibleSection title="Sandbox Defaults" badge="advanced">
              <ToggleField
                label="Sandbox enabled by default"
                description="Run new sessions in a Docker container"
                checked={(sandbox.enabled_by_default as boolean) ?? false}
                onChange={(v) => {
                  setSettings({ ...settings, sandbox: { ...sandbox, enabled_by_default: v } });
                  save("sandbox", { ...sandbox, enabled_by_default: v });
                }}
              />
              <TextField
                label="Default container image"
                value={(sandbox.default_image as string) ?? ""}
                onChange={(v) => {
                  setSettings({ ...settings, sandbox: { ...sandbox, default_image: v } });
                  save("sandbox", { ...sandbox, default_image: v });
                }}
                placeholder="ghcr.io/njbrake/aoe-sandbox:latest"
                mono
              />
              <TextField
                label="CPU limit"
                value={(sandbox.cpu_limit as string) ?? ""}
                onChange={(v) => {
                  setSettings({ ...settings, sandbox: { ...sandbox, cpu_limit: v || null } });
                  save("sandbox", { ...sandbox, cpu_limit: v || null });
                }}
                placeholder="e.g. 4"
              />
              <TextField
                label="Memory limit"
                value={(sandbox.memory_limit as string) ?? ""}
                onChange={(v) => {
                  setSettings({ ...settings, sandbox: { ...sandbox, memory_limit: v || null } });
                  save("sandbox", { ...sandbox, memory_limit: v || null });
                }}
                placeholder="e.g. 8g"
              />
              <ToggleField
                label="Mount SSH keys"
                description="Mount ~/.ssh into sandbox containers"
                checked={(sandbox.mount_ssh as boolean) ?? false}
                onChange={(v) => {
                  setSettings({ ...settings, sandbox: { ...sandbox, mount_ssh: v } });
                  save("sandbox", { ...sandbox, mount_ssh: v });
                }}
              />
              <ToggleField
                label="Auto cleanup"
                description="Remove containers when sessions are deleted"
                checked={(sandbox.auto_cleanup as boolean) ?? true}
                onChange={(v) => {
                  setSettings({ ...settings, sandbox: { ...sandbox, auto_cleanup: v } });
                  save("sandbox", { ...sandbox, auto_cleanup: v });
                }}
              />
            </CollapsibleSection>

            <CollapsibleSection title="Worktree Config" badge="advanced">
              <ToggleField
                label="Worktrees enabled"
                description="Create git worktrees for new sessions"
                checked={(worktree.enabled as boolean) ?? false}
                onChange={(v) => {
                  setSettings({ ...settings, worktree: { ...worktree, enabled: v } });
                  save("worktree", { ...worktree, enabled: v });
                }}
              />
              <TextField
                label="Path template"
                value={(worktree.path_template as string) ?? ""}
                onChange={(v) => {
                  setSettings({ ...settings, worktree: { ...worktree, path_template: v } });
                  save("worktree", { ...worktree, path_template: v });
                }}
                placeholder="../{repo-name}-worktrees/{branch}"
                mono
              />
              <ToggleField
                label="Auto cleanup"
                description="Delete worktrees when sessions are removed"
                checked={(worktree.auto_cleanup as boolean) ?? true}
                onChange={(v) => {
                  setSettings({ ...settings, worktree: { ...worktree, auto_cleanup: v } });
                  save("worktree", { ...worktree, auto_cleanup: v });
                }}
              />
              <ToggleField
                label="Delete branch on cleanup"
                description="Also delete the git branch when cleaning up a worktree"
                checked={(worktree.delete_branch_on_cleanup as boolean) ?? false}
                onChange={(v) => {
                  setSettings({ ...settings, worktree: { ...worktree, delete_branch_on_cleanup: v } });
                  save("worktree", { ...worktree, delete_branch_on_cleanup: v });
                }}
              />
            </CollapsibleSection>
          </>
        )}

        <ConnectedDevices />
      </div>
    </div>
  );
}
