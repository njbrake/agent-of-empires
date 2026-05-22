// @vitest-environment jsdom
//
// Hook-reducer tests for the cockpit config-option (model picker +
// reasoning effort) feature (#1403). Covers the new internal actions
// that drive pending state and the dismissable failure notice. The
// POST shape and the end-to-end pessimistic-update flow are exercised
// by the mocked Playwright spec under web/tests/.

import { describe, expect, it } from "vitest";

import { emptyCockpitState } from "../lib/cockpitTypes";
import { cockpitHookReducer } from "./useCockpit";

describe("cockpitHookReducer / config option actions", () => {
  it("set_pending_config_option records the requested click", () => {
    const next = cockpitHookReducer(emptyCockpitState(), {
      kind: "set_pending_config_option",
      configId: "model",
      value: "claude-sonnet-4-6",
    });
    expect(next.pendingConfigOption).toEqual({
      configId: "model",
      value: "claude-sonnet-4-6",
    });
  });

  it("clear_pending_config_option drops the in-flight record", () => {
    const seeded = cockpitHookReducer(emptyCockpitState(), {
      kind: "set_pending_config_option",
      configId: "effort",
      value: "high",
    });
    const next = cockpitHookReducer(seeded, {
      kind: "clear_pending_config_option",
    });
    expect(next.pendingConfigOption).toBeNull();
  });

  it("dismiss_config_option_switch_failed clears the notice", () => {
    const seeded = {
      ...emptyCockpitState(),
      configOptionSwitchFailed: {
        configId: "model",
        value: "claude-sonnet-4-6",
        reason: "rate limited",
        at: new Date().toISOString(),
      },
    };
    const next = cockpitHookReducer(seeded, {
      kind: "dismiss_config_option_switch_failed",
    });
    expect(next.configOptionSwitchFailed).toBeNull();
  });

  it("set_pending overrides any previous pending click on the same option", () => {
    let state = cockpitHookReducer(emptyCockpitState(), {
      kind: "set_pending_config_option",
      configId: "model",
      value: "claude-opus-4-7",
    });
    state = cockpitHookReducer(state, {
      kind: "set_pending_config_option",
      configId: "model",
      value: "claude-sonnet-4-6",
    });
    expect(state.pendingConfigOption?.value).toBe("claude-sonnet-4-6");
  });
});
