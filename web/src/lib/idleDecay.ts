import type { SettingsResponse } from "./api";
import { IDLE_DECAY_WINDOW_MS } from "./session";

export function resolveIdleDecayWindowMs(
  settings: SettingsResponse | null | undefined,
): number {
  const minutes = settings?.theme?.idle_decay_minutes;
  if (typeof minutes !== "number" || !Number.isFinite(minutes)) {
    return IDLE_DECAY_WINDOW_MS;
  }
  return Math.max(0, minutes) * 60 * 1000;
}
