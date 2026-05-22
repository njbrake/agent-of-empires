import { NumberField, SelectField, ToggleField } from "./FormFields";

interface Props {
  settings: Record<string, unknown>;
  onSaveField: (section: string, field: string, value: unknown) => void;
  onUpdate: (patch: Record<string, unknown>) => void;
}

const MODE_OPTIONS = [
  {
    value: "auto",
    label: "auto - install in background on next launch",
  },
  {
    value: "notify",
    label: "notify - show banner / CLI notice (default)",
  },
  { value: "off", label: "off - skip every check" },
];

const VALID_MODES = new Set(["auto", "notify", "off"]);

export function UpdateSettings({ settings, onSaveField, onUpdate }: Props) {
  const updates = (settings.updates ?? {}) as Record<string, unknown>;

  const save = (field: string, value: unknown) => {
    onUpdate({ updates: { ...updates, [field]: value } });
    onSaveField("updates", field, value);
  };

  const mode = (() => {
    const v = updates.update_check_mode;
    if (typeof v === "string" && VALID_MODES.has(v)) return v;
    return "notify";
  })();

  return (
    <div className="space-y-4">
      <SelectField
        label="Update check mode"
        description="auto installs detected releases in the background and picks them up next launch. notify (default) shows the banner. off skips every check, banner, and fetch."
        value={mode}
        onChange={(v) => save("update_check_mode", v)}
        options={MODE_OPTIONS}
      />
      <NumberField
        label="Check interval (hours)"
        value={(updates.check_interval_hours as number) ?? 24}
        onChange={(v) => save("check_interval_hours", Math.max(1, v))}
        min={1}
      />
      <ToggleField
        label="Notify in CLI"
        description="Show update notifications in the terminal (independent of the TUI banner; only fires while mode = notify)"
        checked={(updates.notify_in_cli as boolean) ?? true}
        onChange={(v) => save("notify_in_cli", v)}
      />
      <NumberField
        label="Web poll interval (minutes)"
        value={(updates.web_poll_interval_minutes as number) ?? 60}
        onChange={(v) =>
          save("web_poll_interval_minutes", Math.max(5, v))
        }
        min={5}
      />
    </div>
  );
}
