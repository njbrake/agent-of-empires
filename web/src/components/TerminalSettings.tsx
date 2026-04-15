import { useWebSettings } from "../hooks/useWebSettings";

const FONT_SIZES = Array.from({ length: 23 }, (_, i) => i + 6); // 6..28

export function TerminalSettings() {
  const { settings, update } = useWebSettings();

  return (
    <div>
      <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-4">
        Terminal
      </h3>

      <div className="space-y-4">
        <div>
          <label className="block text-[13px] text-text-secondary mb-2">
            Mobile font size
          </label>
          <div className="flex items-center gap-3">
            <input
              type="range"
              min={6}
              max={28}
              step={1}
              value={settings.mobileFontSize}
              onChange={(e) =>
                update({ mobileFontSize: Number(e.target.value) })
              }
              className="flex-1 accent-brand-600 h-1.5"
            />
            <select
              value={settings.mobileFontSize}
              onChange={(e) =>
                update({ mobileFontSize: Number(e.target.value) })
              }
              className="bg-surface-800 border border-surface-700 rounded-md px-2 py-1 text-sm text-text-primary font-mono w-16 text-center"
            >
              {FONT_SIZES.map((s) => (
                <option key={s} value={s}>
                  {s}px
                </option>
              ))}
            </select>
          </div>
          <p className="text-[11px] text-text-muted mt-1">
            Font size for the terminal on mobile devices. Pinch the terminal
            with two fingers to zoom; the new size is saved here.
          </p>
        </div>

        <div>
          <label className="flex items-center justify-between gap-3 cursor-pointer">
            <div>
              <div className="text-[13px] text-text-secondary">
                Auto-open keyboard on mobile
              </div>
              <p className="text-[11px] text-text-muted mt-1">
                Open the soft keyboard when you select a session. Turn off for
                monitoring-first workflows.
              </p>
            </div>
            <input
              type="checkbox"
              checked={settings.autoOpenKeyboard}
              onChange={(e) =>
                update({ autoOpenKeyboard: e.target.checked })
              }
              className="accent-brand-600 w-4 h-4 shrink-0"
            />
          </label>
        </div>
      </div>
    </div>
  );
}
