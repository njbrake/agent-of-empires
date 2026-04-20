import { useEffect, useState } from "react";
import type { SessionStatus } from "../lib/types";

/** Animated spinner frames from rattles (https://github.com/vyfor/rattles) */
const RATTLES: Record<string, { frames: string[]; interval: number }> = {
  dots:         { frames: ["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"], interval: 220 },
  orbit:        { frames: ["⠃","⠉","⠘","⠰","⢠","⣀","⡄","⠆"], interval: 400 },
  breathe:      { frames: ["⠀","⠂","⠌","⡑","⢕","⢝","⣫","⣟","⣿","⣟","⣫","⢝","⢕","⡑","⠌","⠂","⠀"], interval: 180 },
};

/** Which statuses get animated spinners vs static glyphs */
const STATUS_RATTLE: Partial<Record<SessionStatus, keyof typeof RATTLES>> = {
  Running: "dots",
  Waiting: "orbit",
  Starting: "breathe",
  Creating: "orbit",
};

/** Static glyphs for non-animated statuses (braille family) */
const STATIC_GLYPH: Record<SessionStatus, string> = {
  Running: "⠋",
  Waiting: "⠃",
  Idle: "⠒",
  Error: "✕",
  Starting: "⠀",
  Stopped: "⠒",
  Unknown: "⠤",
  Deleting: "✕",
  Creating: "⠀",
};

/** Animated status glyph that cycles through rattles frames.
 *  Each instance offsets by `createdAt` so spinners look unique. */
export function StatusGlyph({
  status,
  createdAt,
}: {
  status: SessionStatus;
  createdAt: string | null;
}) {
  const rattleKey = STATUS_RATTLE[status];
  const rattle = rattleKey ? RATTLES[rattleKey] : undefined;
  const parsed = createdAt ? Date.parse(createdAt) : 0;
  const epoch = Number.isNaN(parsed) ? 0 : parsed;
  const [frame, setFrame] = useState(() => {
    if (!rattle) return 0;
    return Math.floor((Date.now() - epoch) / rattle.interval) % rattle.frames.length;
  });

  useEffect(() => {
    if (!rattle) return;
    const r = rattle;
    const computeFrame = () =>
      Math.floor((Date.now() - epoch) / r.interval) % r.frames.length;
    setFrame(computeFrame());
    const id = setInterval(() => setFrame(computeFrame()), r.interval);
    return () => clearInterval(id);
  }, [rattle, epoch]);

  if (!rattle) return <>{STATIC_GLYPH[status]}</>;
  return <>{rattle.frames[frame]}</>;
}
