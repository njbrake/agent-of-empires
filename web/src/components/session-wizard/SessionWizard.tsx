import { useCallback, useEffect, useMemo, useReducer } from "react";
import type { AgentInfo, GroupInfo, ProfileInfo, CreateSessionRequest, SessionResponse } from "../../lib/types";
import { fetchAgents, fetchGroups, fetchDockerStatus, fetchProfiles, getSettings, createSession } from "../../lib/api";
import { StepIndicator } from "./StepIndicator";
import type { StepDef, StepId } from "./StepIndicator";
import { ProjectStep } from "./steps/ProjectStep";
import { AgentStep } from "./steps/AgentStep";
import { ReviewStep } from "./steps/ReviewStep";

export interface WizardData {
  path: string;
  title: string;
  group: string;
  tool: string;
  profile: string;
  yoloMode: boolean;
  sandboxEnabled: boolean;
  sandboxImage: string;
  extraEnv: string[];
  advancedEnabled: boolean;
  customInstruction: string;
  extraArgs: string;
  commandOverride: string;
  /** Tracks whether the user has manually edited fields after a profile selection */
  profileDirty: boolean;
  [key: string]: unknown;
}

interface WizardState {
  currentStep: number;
  data: WizardData;
  isSubmitting: boolean;
  error: string | null;
  agents: AgentInfo[];
  groups: GroupInfo[];
  profiles: ProfileInfo[];
  dockerAvailable: boolean;
}

type Action =
  | { type: "SET_FIELD"; field: string; value: unknown }
  | { type: "SET_STEP"; step: number }
  | { type: "SUBMIT_START" }
  | { type: "SUBMIT_ERROR"; error: string }
  | { type: "SUBMIT_SUCCESS" }
  | { type: "SET_AGENTS"; agents: AgentInfo[] }
  | { type: "SET_GROUPS"; groups: GroupInfo[] }
  | { type: "SET_PROFILES"; profiles: ProfileInfo[] }
  | { type: "SET_DOCKER"; available: boolean }
  | { type: "APPLY_PROFILE_DEFAULTS"; yoloMode: boolean; sandboxEnabled: boolean; tool: string; extraEnv: string[] };

const initialData: WizardData = {
  path: "", title: "", group: "", tool: "claude", profile: "",
  yoloMode: false, sandboxEnabled: false, sandboxImage: "", extraEnv: [],
  advancedEnabled: false, profileDirty: false,
  customInstruction: "", extraArgs: "", commandOverride: "",
};

function reducer(state: WizardState, action: Action): WizardState {
  switch (action.type) {
    case "SET_FIELD": {
      const newData = { ...state.data, [action.field]: action.value };
      // Mark as dirty when user manually edits agent-step fields after a profile was chosen
      if (state.data.profile && ["yoloMode", "sandboxEnabled", "tool", "extraEnv"].includes(action.field)) {
        newData.profileDirty = true;
      }
      return { ...state, data: newData, error: null };
    }
    case "SET_STEP":
      return { ...state, currentStep: action.step };
    case "SUBMIT_START":
      return { ...state, isSubmitting: true, error: null };
    case "SUBMIT_ERROR":
      return { ...state, isSubmitting: false, error: action.error };
    case "SUBMIT_SUCCESS":
      return { ...state, isSubmitting: false };
    case "SET_AGENTS":
      return { ...state, agents: action.agents };
    case "SET_GROUPS":
      return { ...state, groups: action.groups };
    case "SET_PROFILES":
      return { ...state, profiles: action.profiles };
    case "SET_DOCKER":
      return { ...state, dockerAvailable: action.available };
    case "APPLY_PROFILE_DEFAULTS":
      return {
        ...state,
        data: {
          ...state.data,
          yoloMode: action.yoloMode,
          sandboxEnabled: action.sandboxEnabled,
          tool: action.tool || state.data.tool,
          extraEnv: action.extraEnv,
          profileDirty: false,
        },
      };
    default:
      return state;
  }
}

// Wizard: project path → agent/settings → review
function computeSteps(_data: WizardData): StepDef[] {
  return [
    { id: "project", label: "Project" },
    { id: "agent", label: "Settings" },
    { id: "review", label: "Review" },
  ];
}

export interface WizardPrefill {
  path?: string;
  tool?: string;
  yoloMode?: boolean;
  sandboxEnabled?: boolean;
  profile?: string;
  group?: string;
  /** If true, skip to the review step (all fields pre-filled) */
  skipToReview?: boolean;
  /** Which tab to show initially on the project step */
  initialTab?: "recent" | "browse" | "clone";
}

interface Props {
  onClose: () => void;
  onCreated: (session?: SessionResponse) => void;
  prefill?: WizardPrefill;
}

export function SessionWizard({ onClose, onCreated, prefill }: Props) {
  const prefillData: WizardData = prefill
    ? {
        ...initialData,
        path: prefill.path || "",
        tool: prefill.tool || "claude",
        yoloMode: prefill.yoloMode ?? false,
        sandboxEnabled: prefill.sandboxEnabled ?? false,
        profile: prefill.profile || "",
        group: prefill.group || "",
      }
    : initialData;

  const [state, dispatch] = useReducer(reducer, {
    currentStep: prefill?.skipToReview ? 2 : (prefill?.path ? 1 : 0),
    data: prefillData, isSubmitting: false, error: null,
    agents: [], groups: [], profiles: [], dockerAvailable: false,
  });

  const steps = useMemo(() => computeSteps(state.data),
    [state.data.sandboxEnabled, state.data.advancedEnabled]);

  const currentStepDef = steps[state.currentStep];
  const isFirst = state.currentStep === 0;
  const isLast = currentStepDef?.id === "review";

  useEffect(() => {
    fetchAgents().then((a) => dispatch({ type: "SET_AGENTS", agents: a }));
    fetchGroups().then((g) => dispatch({ type: "SET_GROUPS", groups: g }));
    fetchProfiles().then((p) => dispatch({ type: "SET_PROFILES", profiles: p }));
    fetchDockerStatus().then((d) => dispatch({ type: "SET_DOCKER", available: d.available }));
    getSettings().then((s) => {
      if (s) {
        const sandbox = s.sandbox as Record<string, unknown> | undefined;
        const img = (sandbox?.default_image as string) || "";
        if (img) dispatch({ type: "SET_FIELD", field: "sandboxImage", value: img });
      }
    });
  }, []);

  const handleChange = useCallback((field: string, value: unknown) => {
    dispatch({ type: "SET_FIELD", field, value });
  }, []);

  const handleApplyProfileDefaults = useCallback((defaults: { yoloMode: boolean; sandboxEnabled: boolean; tool: string; extraEnv: string[] }) => {
    dispatch({ type: "APPLY_PROFILE_DEFAULTS", ...defaults });
  }, []);

  const goNext = () => { if (state.currentStep < steps.length - 1) dispatch({ type: "SET_STEP", step: state.currentStep + 1 }); };
  const goBack = () => { if (state.currentStep > 0) dispatch({ type: "SET_STEP", step: state.currentStep - 1 }); };
  const jumpTo = (stepId: StepId) => { const idx = steps.findIndex((s) => s.id === stepId); if (idx >= 0) dispatch({ type: "SET_STEP", step: idx }); };

  const handleSubmit = async () => {
    dispatch({ type: "SUBMIT_START" });
    const d = state.data;
    const body: CreateSessionRequest = {
      path: d.path, tool: d.tool,
      title: d.title || undefined, group: d.group || undefined,
      yolo_mode: d.yoloMode,
      worktree_branch: d.title || "",
      create_new_branch: true,
      sandbox: d.sandboxEnabled,
      sandbox_image: d.sandboxEnabled ? d.sandboxImage : undefined,
      extra_env: d.sandboxEnabled && d.extraEnv.length > 0 ? d.extraEnv.filter(Boolean) : undefined,
      extra_args: d.extraArgs || undefined,
      command_override: d.commandOverride || undefined,
      custom_instruction: d.customInstruction || undefined,
      profile: d.profile || undefined,
    };
    const result = await createSession(body);
    if (result.ok) { dispatch({ type: "SUBMIT_SUCCESS" }); onCreated(result.session); }
    else dispatch({ type: "SUBMIT_ERROR", error: result.error || "Unknown error" });
  };

  useEffect(() => {
    if (state.currentStep >= steps.length) dispatch({ type: "SET_STEP", step: steps.length - 1 });
  }, [steps.length, state.currentStep]);

  const renderStep = () => {
    switch (currentStepDef?.id) {
      case "project":
        return <ProjectStep data={state.data} onChange={handleChange} initialTab={prefill?.initialTab} />;
      case "agent":
        return (
          <AgentStep
            data={state.data}
            onChange={handleChange}
            agents={state.agents}
            profiles={state.profiles}
            dockerAvailable={state.dockerAvailable}
            onApplyProfileDefaults={handleApplyProfileDefaults}
          />
        );
      case "review":
        return <ReviewStep data={state.data} isSubmitting={state.isSubmitting} error={state.error} onSubmit={handleSubmit} onJumpTo={jumpTo} steps={steps} />;
      default:
        return null;
    }
  };

  const nextDisabled = currentStepDef?.id === "project" && !state.data.path;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/60" onClick={onClose} />
      <div className="relative w-full max-w-lg bg-surface-800 border border-surface-700/30 rounded-xl flex flex-col max-h-[min(720px,90vh)]">
        <div className="flex items-center justify-between px-5 py-4 border-b border-surface-700/20">
          <h1 className="text-sm font-medium text-text-secondary">Add project</h1>
          <button onClick={onClose} className="w-8 h-8 flex items-center justify-center text-text-dim hover:text-text-secondary cursor-pointer rounded-md hover:bg-surface-700/50 transition-colors" aria-label="Close">&times;</button>
        </div>
        <div className="flex-1 overflow-y-auto px-5 py-5">
          <StepIndicator steps={steps} currentIndex={state.currentStep} />
          {renderStep()}
        </div>
        {!isLast && (
          <div className="flex justify-between px-5 py-4 border-t border-surface-700/20">
            <button onClick={isFirst ? onClose : goBack}
              className="px-5 py-2.5 text-sm rounded-lg border border-surface-700 text-text-secondary hover:bg-surface-800 active:bg-surface-700 cursor-pointer transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-brand-600">
              {isFirst ? "Cancel" : "Back"}
            </button>
            <button onClick={goNext} disabled={nextDisabled}
              className={`px-5 py-2.5 text-sm rounded-lg font-semibold transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-brand-600 ${
                nextDisabled
                  ? "bg-brand-600/50 text-surface-900/50 cursor-not-allowed"
                  : "bg-brand-600 hover:bg-brand-700 active:bg-brand-800 text-surface-900 cursor-pointer"
              }`}>
              Next
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
