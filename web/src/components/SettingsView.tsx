import { useCallback, useEffect, useState } from "react";
import { ConnectedDevices } from "./ConnectedDevices";
import { NotificationSettings } from "./NotificationSettings";
import { SecuritySettings } from "./SecuritySettings";
import { TerminalSettings } from "./TerminalSettings";
import {
  getSettings,
  updateSettings,
  updateProfileSettings,
} from "../lib/api";
import {
  ListField,
  SelectField,
  TextField,
  ToggleField,
} from "./settings/FormFields";
import { ThemeSettings } from "./settings/ThemeSettings";
import { SoundSettings } from "./settings/SoundSettings";
import { UpdateSettings } from "./settings/UpdateSettings";
import { TmuxSettings } from "./settings/TmuxSettings";
import { ProfileSelector } from "./settings/ProfileSelector";

type TabId =
  | "session"
  | "sandbox"
  | "worktree"
  | "theme"
  | "sound"
  | "tmux"
  | "updates"
  | "notifications"
  | "terminal"
  | "security"
  | "devices";

const TABS: { id: TabId; label: string }[] = [
  { id: "session", label: "Session" },
  { id: "sandbox", label: "Sandbox" },
  { id: "worktree", label: "Worktree" },
  { id: "theme", label: "Theme" },
  { id: "sound", label: "Sound" },
  { id: "tmux", label: "Tmux" },
  { id: "updates", label: "Updates" },
  { id: "notifications", label: "Notifications" },
  { id: "terminal", label: "Terminal" },
  { id: "security", label: "Security" },
  { id: "devices", label: "Devices" },
];

interface Props {
  onClose: () => void;
}

export function SettingsView({ onClose }: Props) {
  const [settings, setSettings] = useState<Record<string, unknown> | null>(
    null,
  );
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [selectedProfile, setSelectedProfile] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<TabId>("session");

  const loadSettings = useCallback(() => {
    const loader = selectedProfile
      ? getSettings(selectedProfile)
      : getSettings();
    loader.then((s) => {
      if (s) setSettings(s);
    });
  }, [selectedProfile]);

  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  const sendSave = useCallback(
    async (section: string, data: Record<string, unknown>) => {
      setSaving(true);
      setSaveError(null);
      const ok = selectedProfile
        ? await updateProfileSettings(selectedProfile, { [section]: data })
        : await updateSettings({ [section]: data });
      setSaving(false);
      if (!ok) {
        setSaveError("Failed to save, please try again");
        loadSettings();
      }
    },
    [selectedProfile, loadSettings],
  );

  const updateLocal = useCallback(
    (patch: Record<string, unknown>) => {
      if (settings) setSettings({ ...settings, ...patch });
    },
    [settings],
  );

  const session = (settings?.session ?? {}) as Record<string, unknown>;
  const sandbox = (settings?.sandbox ?? {}) as Record<string, unknown>;
  const worktree = (settings?.worktree ?? {}) as Record<string, unknown>;
  const web = (settings?.web ?? {}) as Record<string, unknown>;

  const saveField = (
    section: string,
    sectionData: Record<string, unknown>,
    field: string,
    value: unknown,
  ) => {
    const updated = { ...sectionData, [field]: value };
    updateLocal({ [section]: updated });
    if (selectedProfile) {
      sendSave(section, { [field]: value });
    } else {
      sendSave(section, updated);
    }
  };

  const saveSubField = useCallback(
    (section: string, field: string, value: unknown) => {
      const sectionData = (settings?.[section] ?? {}) as Record<string, unknown>;
      saveField(section, sectionData, field, value);
    },
    [settings, selectedProfile, sendSave, loadSettings],
  );

  const renderTabContent = () => {
    if (!settings && activeTab !== "notifications" && activeTab !== "terminal" && activeTab !== "security" && activeTab !== "devices") {
      return <div className="text-sm text-text-dim">Loading settings...</div>;
    }

    switch (activeTab) {
      case "session":
        return (
          <div className="space-y-4">
            <TextField
              label="Default agent"
              value={(session.default_tool as string) ?? ""}
              onChange={(v) => saveField("session", session, "default_tool", v || null)}
              placeholder="Auto-detect"
              mono
            />
            <ToggleField
              label="YOLO mode by default"
              description="New sessions skip permission prompts"
              checked={(session.yolo_mode_default as boolean) ?? false}
              onChange={(v) => saveField("session", session, "yolo_mode_default", v)}
            />
            <ToggleField
              label="Strict hotkeys"
              description="Require SHIFT on letter-based TUI hotkeys to prevent accidental actions"
              checked={(session.strict_hotkeys as boolean) ?? false}
              onChange={(v) => saveField("session", session, "strict_hotkeys", v)}
            />
            <ToggleField
              label="Agent status hooks"
              description="Install status-detection hooks into agent settings files for reliable status tracking"
              checked={(session.agent_status_hooks as boolean) ?? true}
              onChange={(v) => saveField("session", session, "agent_status_hooks", v)}
            />
          </div>
        );

      case "sandbox":
        return (
          <div className="space-y-4">
            <ToggleField
              label="Sandbox enabled by default"
              description="Run new sessions in a Docker container"
              checked={(sandbox.enabled_by_default as boolean) ?? false}
              onChange={(v) => saveField("sandbox", sandbox, "enabled_by_default", v)}
            />
            <TextField
              label="Default container image"
              value={(sandbox.default_image as string) ?? ""}
              onChange={(v) => saveField("sandbox", sandbox, "default_image", v)}
              placeholder="ghcr.io/njbrake/aoe-sandbox:latest"
              mono
            />
            <SelectField
              label="Default terminal mode"
              value={(sandbox.default_terminal_mode as string) ?? "host"}
              onChange={(v) => saveField("sandbox", sandbox, "default_terminal_mode", v)}
              options={[
                { value: "host", label: "Host" },
                { value: "container", label: "Container" },
              ]}
            />
            <SelectField
              label="Container runtime"
              value={(sandbox.container_runtime as string) ?? "docker"}
              onChange={(v) => saveField("sandbox", sandbox, "container_runtime", v)}
              options={[
                { value: "docker", label: "Docker" },
                { value: "apple_container", label: "Apple Container" },
              ]}
            />
            <TextField
              label="CPU limit"
              value={(sandbox.cpu_limit as string) ?? ""}
              onChange={(v) => saveField("sandbox", sandbox, "cpu_limit", v || null)}
              placeholder="e.g. 4"
            />
            <TextField
              label="Memory limit"
              value={(sandbox.memory_limit as string) ?? ""}
              onChange={(v) => saveField("sandbox", sandbox, "memory_limit", v || null)}
              placeholder="e.g. 8g"
            />
            <ToggleField
              label="Mount SSH keys"
              description="Mount ~/.ssh into sandbox containers"
              checked={(sandbox.mount_ssh as boolean) ?? false}
              onChange={(v) => saveField("sandbox", sandbox, "mount_ssh", v)}
            />
            <ToggleField
              label="Auto cleanup"
              description="Remove containers when sessions are deleted"
              checked={(sandbox.auto_cleanup as boolean) ?? true}
              onChange={(v) => saveField("sandbox", sandbox, "auto_cleanup", v)}
            />
            <TextField
              label="Custom instruction"
              description="Text appended to the agent system prompt in sandboxed sessions"
              value={(sandbox.custom_instruction as string) ?? ""}
              onChange={(v) => saveField("sandbox", sandbox, "custom_instruction", v || null)}
              placeholder="Additional instructions for the agent..."
              multiline
            />
            <ListField
              label="Environment variables"
              description="Variables passed to sandbox containers (KEY or KEY=VALUE)"
              items={(sandbox.environment as string[]) ?? []}
              onChange={(items) => saveField("sandbox", sandbox, "environment", items)}
              placeholder="KEY or KEY=VALUE"
              validate={(v) => {
                if (!/^[A-Za-z_][A-Za-z0-9_]*(=.*)?$/.test(v))
                  return "Must be KEY or KEY=VALUE (letters, digits, underscores)";
                return null;
              }}
            />
            <ListField
              label="Extra volumes"
              description="Additional volume mounts (host:container[:ro])"
              items={(sandbox.extra_volumes as string[]) ?? []}
              onChange={(items) => saveField("sandbox", sandbox, "extra_volumes", items)}
              placeholder="/host/path:/container/path"
              validate={(v) => {
                if (!v.includes(":")) return "Must contain ':' (host:container)";
                return null;
              }}
            />
            <ListField
              label="Port mappings"
              description="Port forwarding (host:container)"
              items={(sandbox.port_mappings as string[]) ?? []}
              onChange={(items) => saveField("sandbox", sandbox, "port_mappings", items)}
              placeholder="3000:3000"
              validate={(v) => {
                if (!/^\d+:\d+$/.test(v)) return "Must be port:port (e.g. 3000:3000)";
                return null;
              }}
            />
            <ListField
              label="Volume ignores"
              description="Directories excluded from host bind mount"
              items={(sandbox.volume_ignores as string[]) ?? []}
              onChange={(items) => saveField("sandbox", sandbox, "volume_ignores", items)}
              placeholder="node_modules"
            />
          </div>
        );

      case "worktree":
        return (
          <div className="space-y-4">
            <ToggleField
              label="Worktrees enabled"
              description="Create git worktrees for new sessions"
              checked={(worktree.enabled as boolean) ?? false}
              onChange={(v) => saveField("worktree", worktree, "enabled", v)}
            />
            <TextField
              label="Path template"
              value={(worktree.path_template as string) ?? ""}
              onChange={(v) => saveField("worktree", worktree, "path_template", v)}
              placeholder="../{repo-name}-worktrees/{branch}"
              mono
            />
            <TextField
              label="Bare repo path template"
              value={(worktree.bare_repo_path_template as string) ?? ""}
              onChange={(v) => saveField("worktree", worktree, "bare_repo_path_template", v)}
              placeholder="./{branch}"
              mono
            />
            <TextField
              label="Workspace path template"
              value={(worktree.workspace_path_template as string) ?? ""}
              onChange={(v) => saveField("worktree", worktree, "workspace_path_template", v)}
              placeholder="../{branch}-workspace-{session-id}"
              mono
            />
            <ToggleField
              label="Auto cleanup"
              description="Delete worktrees when sessions are removed"
              checked={(worktree.auto_cleanup as boolean) ?? true}
              onChange={(v) => saveField("worktree", worktree, "auto_cleanup", v)}
            />
            <ToggleField
              label="Delete branch on cleanup"
              description="Also delete the git branch when cleaning up a worktree"
              checked={(worktree.delete_branch_on_cleanup as boolean) ?? false}
              onChange={(v) => saveField("worktree", worktree, "delete_branch_on_cleanup", v)}
            />
          </div>
        );

      case "theme":
        return <ThemeSettings settings={settings!} onSaveField={saveSubField} onUpdate={updateLocal} />;
      case "sound":
        return <SoundSettings settings={settings!} onSaveField={saveSubField} onUpdate={updateLocal} />;
      case "tmux":
        return <TmuxSettings settings={settings!} onSaveField={saveSubField} onUpdate={updateLocal} />;
      case "updates":
        return <UpdateSettings settings={settings!} onSaveField={saveSubField} onUpdate={updateLocal} />;

      case "notifications":
        return (
          <div className="space-y-6">
            <NotificationSettings />
            {settings && (
              <div className="space-y-4">
                <h4 className="text-xs font-mono uppercase tracking-widest text-text-muted">
                  Server Defaults
                </h4>
                <p className="text-xs text-text-dim">
                  Controls which session events trigger push notifications on the server.
                </p>
                <ToggleField
                  label="Push notifications enabled"
                  description="Server-wide kill switch for push notifications"
                  checked={(web.notifications_enabled as boolean) ?? true}
                  onChange={(v) => saveField("web", web, "notifications_enabled", v)}
                />
                <ToggleField
                  label="Notify on waiting"
                  description="Send push when a session needs input"
                  checked={(web.notify_on_waiting as boolean) ?? true}
                  onChange={(v) => saveField("web", web, "notify_on_waiting", v)}
                />
                <ToggleField
                  label="Notify on idle"
                  description="Send push when a session finishes"
                  checked={(web.notify_on_idle as boolean) ?? false}
                  onChange={(v) => saveField("web", web, "notify_on_idle", v)}
                />
                <ToggleField
                  label="Notify on error"
                  description="Send push when a session errors"
                  checked={(web.notify_on_error as boolean) ?? true}
                  onChange={(v) => saveField("web", web, "notify_on_error", v)}
                />
              </div>
            )}
          </div>
        );

      case "terminal":
        return <TerminalSettings />;
      case "security":
        return <SecuritySettings />;
      case "devices":
        return <ConnectedDevices />;
    }
  };

  const currentTabLabel = TABS.find((t) => t.id === activeTab)?.label ?? "";

  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-surface-900">
      {/* Header */}
      <div className="h-12 bg-surface-850 border-b border-surface-700 flex items-center px-4 shrink-0">
        <button
          onClick={onClose}
          className="text-brand-500 mr-3 cursor-pointer text-sm"
        >
          &larr; Back
        </button>
        <span className="text-sm font-semibold text-text-bright">Settings</span>
        {saving && (
          <span className="ml-2 text-xs text-text-dim">Saving...</span>
        )}
        {saveError && (
          <span className="ml-2 text-xs text-red-400">{saveError}</span>
        )}
      </div>

      {/* Mobile tabs (horizontal scroll) */}
      <div className="md:hidden border-b border-surface-700 bg-surface-850 overflow-x-auto">
        <div className="flex">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`px-4 py-2.5 text-xs font-medium whitespace-nowrap cursor-pointer transition-colors ${
                activeTab === tab.id
                  ? "text-brand-500 border-b-2 border-brand-500"
                  : "text-text-dim hover:text-text-primary"
              }`}
            >
              {tab.label}
            </button>
          ))}
        </div>
      </div>

      {/* Desktop: sidebar tabs + content */}
      <div className="flex-1 flex min-h-0">
        {/* Side tabs (desktop only) */}
        <nav className="hidden md:flex flex-col w-44 shrink-0 border-r border-surface-700 bg-surface-850 py-2 overflow-y-auto">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`px-4 py-2 text-sm text-left cursor-pointer transition-colors ${
                activeTab === tab.id
                  ? "text-brand-500 bg-surface-800 border-r-2 border-brand-500"
                  : "text-text-dim hover:text-text-primary hover:bg-surface-800/50"
              }`}
            >
              {tab.label}
            </button>
          ))}
        </nav>

        {/* Content area */}
        <div className="flex-1 overflow-y-auto">
          <div className="p-6 max-w-2xl mx-auto space-y-5">
            {/* Profile selector + tab heading */}
            <ProfileSelector
              selectedProfile={selectedProfile}
              onSelect={setSelectedProfile}
            />

            <h2 className="text-lg font-semibold text-text-bright">{currentTabLabel}</h2>

            {renderTabContent()}
          </div>
        </div>
      </div>
    </div>
  );
}
