import { CollapsibleSection, SelectField } from "./FormFields";

interface Props {
  settings: Record<string, unknown>;
  onSave: (section: string, data: Record<string, unknown>) => void;
  onUpdate: (patch: Record<string, unknown>) => void;
}

export function TmuxSettings({ settings, onSave, onUpdate }: Props) {
  const tmux = (settings.tmux ?? {}) as Record<string, unknown>;

  const save = (field: string, value: unknown) => {
    const updated = { ...tmux, [field]: value };
    onUpdate({ tmux: updated });
    onSave("tmux", updated);
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
