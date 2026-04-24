import { CollapsibleSection, SelectField } from "./FormFields";

interface Props {
  settings: Record<string, unknown>;
  onSaveField: (section: string, field: string, value: unknown) => void;
  onUpdate: (patch: Record<string, unknown>) => void;
}

export function TmuxSettings({ settings, onSaveField, onUpdate }: Props) {
  const tmux = (settings.tmux ?? {}) as Record<string, unknown>;

  const save = (field: string, value: unknown) => {
    onUpdate({ tmux: { ...tmux, [field]: value } });
    onSaveField("tmux", field, value);
  };

  const modeOptions = [
    { value: "auto", label: "Auto" },
    { value: "enabled", label: "Enabled" },
    { value: "disabled", label: "Disabled" },
  ];

  return (
    <CollapsibleSection
      title="Tmux"
      subtitle="These settings apply to the TUI (local tmux sessions), not the web dashboard."
    >
      <SelectField
        label="Status bar"
        description="Show tmux status bar in sessions"
        value={(tmux.status_bar as string) ?? "auto"}
        onChange={(v) => save("status_bar", v)}
        options={modeOptions}
      />
      <SelectField
        label="Mouse support"
        description="Enable mouse in tmux sessions"
        value={(tmux.mouse as string) ?? "auto"}
        onChange={(v) => save("mouse", v)}
        options={modeOptions}
      />
    </CollapsibleSection>
  );
}
