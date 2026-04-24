import { SelectField, SliderField, TextField, ToggleField } from "./FormFields";

interface Props {
  settings: Record<string, unknown>;
  onSaveField: (section: string, field: string, value: unknown) => void;
  onUpdate: (patch: Record<string, unknown>) => void;
}

export function SoundSettings({ settings, onSaveField, onUpdate }: Props) {
  const sound = (settings.sound ?? {}) as Record<string, unknown>;

  const save = (field: string, value: unknown) => {
    onUpdate({ sound: { ...sound, [field]: value } });
    onSaveField("sound", field, value);
  };

  const enabled = (sound.enabled as boolean) ?? false;

  return (
    <div className="space-y-4">
      <p className="text-xs text-text-dim">
        Audio alerts play on the server host machine, not in your browser.
      </p>
      <ToggleField
        label="Enabled"
        description="Play sounds on session status changes"
        checked={enabled}
        onChange={(v) => save("enabled", v)}
      />
      {enabled && (
        <>
          <SelectField
            label="Mode"
            value={
              typeof sound.mode === "string"
                ? sound.mode
                : typeof sound.mode === "object" && sound.mode !== null
                  ? "specific"
                  : "random"
            }
            onChange={(v) =>
              save("mode", v === "random" ? "random" : { specific: "" })
            }
            options={[
              { value: "random", label: "Random" },
              { value: "specific", label: "Specific" },
            ]}
          />
          <SliderField
            label="Volume"
            value={(sound.volume as number) ?? 1.0}
            onChange={(v) => save("volume", v)}
            min={0.1}
            max={1.5}
            step={0.1}
            formatValue={(v) => v.toFixed(1)}
          />
          <TextField
            label="On start"
            description="Sound file for session start"
            value={(sound.on_start as string) ?? ""}
            onChange={(v) => save("on_start", v || null)}
            placeholder="e.g. startup.wav"
            mono
          />
          <TextField
            label="On waiting"
            description="Sound when session needs input"
            value={(sound.on_waiting as string) ?? ""}
            onChange={(v) => save("on_waiting", v || null)}
            placeholder="e.g. waiting.wav"
            mono
          />
          <TextField
            label="On error"
            description="Sound when session errors"
            value={(sound.on_error as string) ?? ""}
            onChange={(v) => save("on_error", v || null)}
            placeholder="e.g. error.wav"
            mono
          />
        </>
      )}
    </div>
  );
}
