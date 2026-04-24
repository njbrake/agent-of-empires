import { NumberField, ToggleField } from "./FormFields";

interface Props {
  settings: Record<string, unknown>;
  onSaveField: (section: string, field: string, value: unknown) => void;
  onUpdate: (patch: Record<string, unknown>) => void;
}

export function UpdateSettings({ settings, onSaveField, onUpdate }: Props) {
  const updates = (settings.updates ?? {}) as Record<string, unknown>;

  const save = (field: string, value: unknown) => {
    onUpdate({ updates: { ...updates, [field]: value } });
    onSaveField("updates", field, value);
  };

  return (
    <div className="space-y-4">
      <ToggleField
        label="Check for updates"
        description="Periodically check for new versions"
        checked={(updates.check_enabled as boolean) ?? true}
        onChange={(v) => save("check_enabled", v)}
      />
      <ToggleField
        label="Auto update"
        description="Automatically install updates when available"
        checked={(updates.auto_update as boolean) ?? false}
        onChange={(v) => save("auto_update", v)}
      />
      <NumberField
        label="Check interval (hours)"
        value={(updates.check_interval_hours as number) ?? 24}
        onChange={(v) => save("check_interval_hours", Math.max(1, v))}
        min={1}
      />
      <ToggleField
        label="Notify in CLI"
        description="Show update notifications in the terminal"
        checked={(updates.notify_in_cli as boolean) ?? true}
        onChange={(v) => save("notify_in_cli", v)}
      />
    </div>
  );
}
