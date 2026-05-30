// The only module that imports react-joyride. Lazy-loaded by TourProvider the
// first time a tour actually runs, so returning users (who have the
// `aoe-tour-seen` flag set) never download the engine. Everything react-joyride
// specific (the component, its event/action constants, theming) lives here;
// TourProvider stays engine-agnostic and deals only in TourStep data. Swapping
// the engine later means rewriting this file alone.
import { useCallback, useMemo } from "react";
import {
  Joyride,
  EVENTS,
  STATUS,
  type ButtonType,
  type EventData,
  type Options,
  type Step,
  type Styles,
} from "react-joyride";
import { type TourStep, tourSelector } from "../../lib/tourSteps";

export interface TourRunnerProps {
  run: boolean;
  steps: TourStep[];
  /** Called once when the tour ends. `markSeen` is false for our own programmatic
   *  stop (scope change / unmount), true for a user finish, skip, or close. */
  onFinish: (markSeen: boolean) => void;
}

// Concrete DESIGN.md tokens (the engine needs hex, not Tailwind classes):
// surface-800 #1e293b card, surface-700 #334155 border, surface-950 #020617
// backdrop, brand-600 #d97706 primary, text-primary #e2e8f0.
const OPTIONS: Partial<Options> = {
  buttons: ["skip", "back", "primary"] as ButtonType[],
  showProgress: true,
  skipBeacon: true,
  primaryColor: "#d97706",
  overlayColor: "rgba(2, 6, 23, 0.65)",
  textColor: "#e2e8f0",
  zIndex: 10_000,
  scrollOffset: 96,
};

const LOCALE = { skip: "Skip", last: "Done", next: "Next", back: "Back" };

const STYLES: Partial<Styles> = {
  tooltip: {
    backgroundColor: "#1e293b",
    border: "1px solid #334155",
    borderRadius: 10,
    color: "#e2e8f0",
    fontSize: 13,
  },
  tooltipTitle: { color: "#f59e0b", fontSize: 14, fontWeight: 600 },
  tooltipContent: { padding: "10px 4px" },
  buttonPrimary: { backgroundColor: "#d97706", borderRadius: 6, color: "#0f172a" },
  buttonBack: { color: "#94a3b8" },
  buttonSkip: { color: "#64748b" },
};

function StepBody({
  body,
  shortcuts,
}: {
  body: string;
  shortcuts?: readonly string[];
}) {
  return (
    <div>
      <p>{body}</p>
      {shortcuts && shortcuts.length > 0 && (
        <ul className="mt-2 space-y-0.5 text-[11px] text-text-muted">
          {shortcuts.map((s) => (
            <li key={s} className="font-mono">
              {s}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function toJoyrideStep(step: TourStep): Step {
  return {
    id: step.id,
    target: tourSelector(step.anchor),
    title: step.title,
    content: <StepBody body={step.body} shortcuts={step.shortcuts} />,
    placement: "auto",
  };
}

export default function TourRunner({ run, steps, onFinish }: TourRunnerProps) {
  const joyrideSteps = useMemo(() => steps.map(toJoyrideStep), [steps]);

  const handleEvent = useCallback(
    (data: EventData) => {
      if (data.type !== EVENTS.TOUR_END) return;
      // Gate on the terminal status, not the action: a programmatic stop
      // (run -> false on scope change / unmount) ends with a non-terminal
      // status and may carry `action: null`, which an action allowlist would
      // misread as a user finish and silently opt the user out. Only an
      // actual finish or skip marks the tour seen.
      const markSeen =
        data.status === STATUS.FINISHED || data.status === STATUS.SKIPPED;
      onFinish(markSeen);
    },
    [onFinish],
  );

  return (
    <Joyride
      run={run}
      steps={joyrideSteps}
      continuous
      options={OPTIONS}
      locale={LOCALE}
      styles={STYLES}
      onEvent={handleEvent}
    />
  );
}
