import { useCallback, useEffect, useMemo, useReducer } from "react";
import type { AgentInfo, GroupInfo, CreateSessionRequest } from "../../lib/types";
import { fetchAgents, fetchGroups, fetchDockerStatus, getSettings, createSession } from "../../lib/api";
import { StepIndicator } from "./StepIndicator";
import type { StepDef, StepId } from "./StepIndicator";
import { ProjectStep } from "./steps/ProjectStep";
import { AgentStep } from "./steps/AgentStep";
import { ContainerStep } from "./steps/ContainerStep";
import { AdvancedStep } from "./steps/AdvancedStep";
import { ReviewStep } from "./steps/ReviewStep";

interface WizardData {
  path: string;
  title: string;
  group: string;
  tool: string;
  yoloMode: boolean;
  sandboxEnabled: boolean;
  sandboxImage: string;
  extraEnv: string[];
  advancedEnabled: boolean;
  customInstruction: string;
  extraArgs: string;
  commandOverride: string;
  [key: string]: unknown;
}

interface WizardState {
  currentStep: number;
  data: WizardData;
  isSubmitting: boolean;
  error: string | null;
  agents: AgentInfo[];
  groups: GroupInfo[];
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
  | { type: "SET_DOCKER"; available: boolean };

const initialData: WizardData = {
  path: "", title: "", group: "", tool: "claude",
  yoloMode: false, sandboxEnabled: false, sandboxImage: "", extraEnv: [],
  advancedEnabled: false,
  customInstruction: "", extraArgs: "", commandOverride: "",
};

function reducer(state: WizardState, action: Action): WizardState {
  switch (action.type) {
    case "SET_FIELD":
      return { ...state, data: { ...state.data, [action.field]: action.value }, error: null };
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
    case "SET_DOCKER":
      return { ...state, dockerAvailable: action.available };
    default:
      return state;
  }
}

// Add Project wizard: project path → agent/settings → container? → advanced? → review
function computeSteps(data: WizardData): StepDef[] {
  const steps: StepDef[] = [
    { id: "project", label: "Project" },
    { id: "agent", label: "Settings" },
  ];
  if (data.sandboxEnabled) steps.push({ id: "container", label: "Container" });
  if (data.advancedEnabled) steps.push({ id: "advanced", label: "Advanced" });
  steps.push({ id: "review", label: "Review" });
  return steps;
}

interface Props {
  onClose: () => void;
  onCreated: () => void;
}

export function SessionWizard({ onClose, onCreated }: Props) {
  const [state, dispatch] = useReducer(reducer, {
    currentStep: 0, data: initialData, isSubmitting: false, error: null,
    agents: [], groups: [], dockerAvailable: false,
  });

  const steps = useMemo(() => computeSteps(state.data),
    [state.data.sandboxEnabled, state.data.advancedEnabled]);

  const currentStepDef = steps[state.currentStep];
  const isFirst = state.currentStep === 0;
  const isLast = currentStepDef?.id === "review";

  useEffect(() => {
    fetchAgents().then((a) => dispatch({ type: "SET_AGENTS", agents: a }));
    fetchGroups().then((g) => dispatch({ type: "SET_GROUPS", groups: g }));
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
    };
    const result = await createSession(body);
    if (result.ok) { dispatch({ type: "SUBMIT_SUCCESS" }); onCreated(); }
    else dispatch({ type: "SUBMIT_ERROR", error: result.error || "Unknown error" });
  };

  useEffect(() => {
    if (state.currentStep >= steps.length) dispatch({ type: "SET_STEP", step: steps.length - 1 });
  }, [steps.length, state.currentStep]);

  const renderStep = () => {
    switch (currentStepDef?.id) {
      case "project":
        return <ProjectStep data={state.data} onChange={handleChange} />;
      case "agent":
        return <AgentStep data={state.data} onChange={handleChange} agents={state.agents} dockerAvailable={state.dockerAvailable} />;
      case "container":
        return <ContainerStep data={state.data} onChange={handleChange} />;
      case "advanced":
        return <AdvancedStep data={state.data} onChange={handleChange} />;
      case "review":
        return <ReviewStep data={state.data} isSubmitting={state.isSubmitting} error={state.error} onSubmit={handleSubmit} onJumpTo={jumpTo} steps={steps} />;
      default:
        return null;
    }
  };

  const nextDisabled = currentStepDef?.id === "project" && !state.data.path;

  return (
    <div className="fixed inset-0 z-50 flex justify-end">
      <div className="absolute inset-0 bg-black/60" onClick={onClose} />
      <div className="relative w-full max-w-lg bg-surface-800 border-l border-surface-700/30 flex flex-col h-full">
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
