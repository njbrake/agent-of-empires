// Tests for the SessionWizard reducer's APPLY_PROFILE_DEFAULTS path,
// added in #1142 so the web wizard now seeds yoloMode/sandboxEnabled/
// tool/extraEnv from the active profile on mount instead of waiting for
// the user to flip the (often-hidden) profile picker.
//
// The reducer is the seam: the mount-time effect dispatches the same
// action the picker does, so unit-testing the reducer covers the
// per-field merge rules without standing up React + the wizard fetch
// graph.

import { describe, expect, it } from "vitest";

import { initialData, reducer, type WizardState } from "./wizardReducer";

function makeState(overrides: Partial<WizardState> = {}): WizardState {
  return {
    currentStep: 0,
    data: { ...initialData },
    isSubmitting: false,
    error: null,
    agents: [],
    groups: [],
    profiles: [],
    dockerAvailable: false,
    ...overrides,
  };
}

describe("SessionWizard reducer / APPLY_PROFILE_DEFAULTS (#1142)", () => {
  it("seeds yoloMode from a profile-resolved fetch on mount", () => {
    // Simulates the mount-time path: the user never touched the picker,
    // and /api/settings?profile=<active> resolved with yolo_mode_default
    // = true. Before #1142 the wizard ignored this and stayed at false.
    const next = reducer(makeState(), {
      type: "APPLY_PROFILE_DEFAULTS",
      yoloMode: true,
      sandboxEnabled: false,
      tool: "claude",
      extraEnv: [],
      skipIfDirty: true,
    });
    expect(next.data.yoloMode).toBe(true);
    expect(next.data.sandboxEnabled).toBe(false);
    expect(next.data.tool).toBe("claude");
    expect(next.data.profileDirty).toBe(false);
  });

  it("seeds sandboxEnabled and extraEnv together so the env list survives", () => {
    const next = reducer(makeState(), {
      type: "APPLY_PROFILE_DEFAULTS",
      yoloMode: false,
      sandboxEnabled: true,
      tool: "claude",
      extraEnv: ["FOO=1", "BAR=baz"],
      skipIfDirty: true,
    });
    expect(next.data.sandboxEnabled).toBe(true);
    expect(next.data.extraEnv).toEqual(["FOO=1", "BAR=baz"]);
  });

  it("falls back to the existing tool when the profile reports an empty default_tool", () => {
    // `(session?.default_tool as string) || ""` resolves empty when the
    // profile doesn't set a tool; the reducer must keep whatever the
    // wizard already had (the prefill or "claude" default).
    const next = reducer(makeState({ data: { ...initialData, tool: "opencode" } }), {
      type: "APPLY_PROFILE_DEFAULTS",
      yoloMode: false,
      sandboxEnabled: false,
      tool: "",
      extraEnv: [],
      skipIfDirty: true,
    });
    expect(next.data.tool).toBe("opencode");
  });

  it("respects skipIfDirty: a slow mount fetch must not clobber user edits", () => {
    // The race the reducer guards against: the user toggled yoloMode off
    // (after picking a profile) before /api/settings resolved. The
    // mount-time dispatch sets skipIfDirty so the late response is a
    // no-op instead of stomping back to the profile default.
    const dirty = makeState({
      data: {
        ...initialData,
        profile: "team-defaults",
        profileDirty: true,
        yoloMode: false,
      },
    });
    const next = reducer(dirty, {
      type: "APPLY_PROFILE_DEFAULTS",
      yoloMode: true,
      sandboxEnabled: true,
      tool: "claude",
      extraEnv: ["FOO=1"],
      skipIfDirty: true,
    });
    expect(next).toBe(dirty);
  });

  it("ignores skipIfDirty for the picker-driven path so confirmed overrides apply", () => {
    // `AgentStep.handleProfileChange` shows a window.confirm() before
    // dispatching with skipIfDirty omitted/false. Even with
    // profileDirty: true, the action must apply.
    const dirty = makeState({
      data: {
        ...initialData,
        profile: "team-defaults",
        profileDirty: true,
        yoloMode: false,
      },
    });
    const next = reducer(dirty, {
      type: "APPLY_PROFILE_DEFAULTS",
      yoloMode: true,
      sandboxEnabled: true,
      tool: "claude",
      extraEnv: [],
    });
    expect(next.data.yoloMode).toBe(true);
    expect(next.data.profileDirty).toBe(false);
  });
});
