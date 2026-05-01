import { useEffect, useState } from "react";
import type { SessionStatus } from "../lib/types";
import { isFreshIdle } from "../lib/session";

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

/** Slowed-down `breathe` rattle for a freshly-stopped Idle session.
 *  Reuses the same animation as Starting on purpose; differentiation is by
 *  color (Starting=dimmed, fresh-idle=amber gradient). The longer interval
 *  (vs Starting) reads as "gentle reminder" rather than "actively
 *  transitioning". */
const FRESH_IDLE_RATTLE = { frames: RATTLES.breathe!.frames, interval: 280 };

/** Animated status glyph that cycles through rattles frames.
 *  Each instance offsets by `createdAt` so spinners look unique.
 *
 *  When `idleEnteredAt` is within the gradient window, an Idle session
 *  renders an animated `breathe` rattle in the gradient color, matching
 *  the visual language of the other attention-worthy states (Running,
 *  Waiting, Starting all animate). Without the rattle the row would be the
 *  only static-glyph state in the "needs attention" bucket, which reads
 *  inconsistent. The motion also serves as a redundant cue alongside the
 *  color decay for colorblind users / monochrome terminals. */
export function StatusGlyph({
  status,
  createdAt,
  idleEnteredAt,
}: {
  status: SessionStatus;
  createdAt: string | null;
  idleEnteredAt?: string | null;
}) {
  const isFresh =
    status === "Idle" && isFreshIdle({ status, idle_entered_at: idleEnteredAt ?? null });
  const rattleKey = STATUS_RATTLE[status];
  const rattle = isFresh
    ? FRESH_IDLE_RATTLE
    : rattleKey
      ? RATTLES[rattleKey]
      : undefined;
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

  if (!rattle) {
    return <>{STATIC_GLYPH[status]}</>;
  }
  return <>{rattle.frames[frame]}</>;
}
