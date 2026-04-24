import { useEffect, useState } from "react";
import { fetchThemes } from "../../lib/api";
import { SelectField } from "./FormFields";

interface Props {
  settings: Record<string, unknown>;
  onSaveField: (section: string, field: string, value: unknown) => void;
  onUpdate: (patch: Record<string, unknown>) => void;
}

export function ThemeSettings({ settings, onSaveField, onUpdate }: Props) {
  const [themes, setThemes] = useState<string[]>([]);
  const theme = (settings.theme ?? {}) as Record<string, unknown>;

  useEffect(() => {
    fetchThemes().then(setThemes);
  }, []);

  const save = (field: string, value: unknown) => {
    onUpdate({ theme: { ...theme, [field]: value } });
    onSaveField("theme", field, value);
  };

  return (
    <div className="space-y-4">
      <p className="text-xs text-text-dim">
        These settings apply to the TUI (local tmux sessions), not the web dashboard.
      </p>
      <SelectField
        label="Theme"
        value={(theme.name as string) ?? ""}
        onChange={(v) => save("name", v)}
        options={[
          { value: "", label: "Default" },
          ...themes.map((t) => ({ value: t, label: t })),
        ]}
      />
      <SelectField
        label="Color mode"
        value={(theme.color_mode as string) ?? "truecolor"}
        onChange={(v) => save("color_mode", v)}
        options={[
          { value: "truecolor", label: "Truecolor (24-bit RGB)" },
          { value: "palette", label: "Palette (256 colors)" },
        ]}
      />
    </div>
  );
}
