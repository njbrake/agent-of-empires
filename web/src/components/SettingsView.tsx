import { ConnectedDevices } from "./ConnectedDevices";
import { SecuritySettings } from "./SecuritySettings";
import { TerminalSettings } from "./TerminalSettings";

interface Props {
  onClose: () => void;
}

export function SettingsView({ onClose }: Props) {
  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-surface-900">
      <div className="h-12 bg-surface-850 border-b border-surface-700 flex items-center px-4 shrink-0">
        <button
          onClick={onClose}
          className="text-brand-500 mr-3 cursor-pointer text-sm"
        >
          &larr; Back
        </button>
        <span className="text-sm font-semibold text-text-bright">
          Settings
        </span>
      </div>

      <div className="flex-1 overflow-y-auto p-6 max-w-[600px] space-y-8">
        <SecuritySettings />
        <TerminalSettings />
        <ConnectedDevices />
      </div>
    </div>
  );
}
