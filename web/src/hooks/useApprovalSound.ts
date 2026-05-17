// Browser-side approval sound for the cockpit.
//
// When the cockpit transitions from "no pending approvals" to "at least
// one pending approval", play the configured sound (from `[sound]` in
// the daemon's config) in the browser. The host-side sound module only
// fires on session-status transitions and runs on the server host, so
// it is the wrong side of the wire when the dashboard is on a separate
// machine. See #1038.
//
// Trade-offs:
//   - Autoplay policies: the first play() may reject if the user has
//     not interacted with the dashboard yet. We swallow the rejection;
//     the OS push notification and in-app toast still surface the
//     approval, so the missing sound is graceful, not fatal.
//   - Auth: `<audio src>` cannot carry an Authorization header, so the
//     blob is fetched through the authenticated fetch path and handed
//     to Audio via `URL.createObjectURL`.

import { useEffect, useRef } from "react";
import { fetchSettings, fetchSounds, fetchSoundBlob } from "../lib/api";

interface SoundSettings {
  enabled?: boolean;
  volume?: number;
  mode?: "random" | { specific: string };
  on_approval?: string | null;
}

interface CachedSound {
  name: string;
  url: string;
}

let cachedSettings: SoundSettings | null = null;
let cachedSettingsAt = 0;
const SETTINGS_TTL_MS = 30_000;

let cachedSound: CachedSound | null = null;

async function loadSettings(): Promise<SoundSettings | null> {
  const now = Date.now();
  if (cachedSettings && now - cachedSettingsAt < SETTINGS_TTL_MS) {
    return cachedSettings;
  }
  const data = await fetchSettings();
  const sound = data?.sound as SoundSettings | undefined;
  cachedSettings = sound ?? null;
  cachedSettingsAt = now;
  return cachedSettings;
}

async function resolveSoundName(
  sound: SoundSettings,
): Promise<string | null> {
  const override = sound.on_approval?.trim();
  if (override) return override;
  if (typeof sound.mode === "object" && sound.mode !== null) {
    const specific = sound.mode.specific?.trim();
    if (specific) return specific;
  }
  if (sound.mode === "random") {
    const list = await fetchSounds();
    if (list.length > 0) {
      return list[Math.floor(Math.random() * list.length)] ?? null;
    }
  }
  return null;
}

async function ensureSoundUrl(name: string): Promise<string | null> {
  if (cachedSound && cachedSound.name === name) {
    return cachedSound.url;
  }
  const blob = await fetchSoundBlob(name);
  if (!blob) return null;
  if (cachedSound) {
    URL.revokeObjectURL(cachedSound.url);
  }
  const url = URL.createObjectURL(blob);
  cachedSound = { name, url };
  return url;
}

async function playApprovalSound(): Promise<void> {
  const sound = await loadSettings();
  if (!sound || !sound.enabled) return;
  const name = await resolveSoundName(sound);
  if (!name) return;
  const url = await ensureSoundUrl(name);
  if (!url) return;
  const audio = new Audio(url);
  const volume = typeof sound.volume === "number" ? sound.volume : 1.0;
  audio.volume = Math.max(0, Math.min(1, volume / 1.5));
  try {
    await audio.play();
  } catch {
    // Autoplay policy or backgrounded tab. The push and in-app toast
    // still cover the user-visible signal; the missing chime is not
    // worth a retry storm.
  }
}

/** Watch `pendingCount` for a 0 -> >=1 edge and play the configured
 *  approval sound. Pure passive: the hook does not mount any UI and
 *  has no return value. */
export function useApprovalSound(pendingCount: number): void {
  const lastCount = useRef(pendingCount);
  useEffect(() => {
    const prev = lastCount.current;
    lastCount.current = pendingCount;
    if (prev === 0 && pendingCount > 0) {
      void playApprovalSound();
    }
  }, [pendingCount]);
}
