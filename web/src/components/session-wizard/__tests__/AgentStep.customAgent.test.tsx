// @vitest-environment jsdom
//
// Covers the AgentStep + ReviewStep changes that opened up the wizard
// to custom-agent selections (#1252):
//
//   - AgentStep selects both kind="custom" entries and `installed`
//     built-ins for the picker grid.
//   - AgentStep renders a "Custom" badge for kind="custom".
//   - AgentStep's SubstrateNotice branches to a custom-agent string
//     when the selected agent's kind is "custom".
//   - ReviewStep's AgentReviewValue renders just the name for built-in
//     agents but adds a "Custom" badge for kind="custom".
//
// Vitest is sufficient here because the changed surface is pure
// rendering; the live persistence path is covered separately by
// web/tests/wizard-custom-agent.spec.ts.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, fireEvent } from "@testing-library/react";

import { AgentStep } from "../steps/AgentStep";
import { ReviewStep } from "../steps/ReviewStep";
import { initialData } from "../wizardReducer";
import type { AgentInfo, ProfileInfo } from "../../../lib/types";

vi.mock("../../../lib/api", () => ({
  fetchSettings: vi.fn().mockResolvedValue({}),
}));

afterEach(() => {
  cleanup();
});

const builtin: AgentInfo = {
  kind: "builtin",
  name: "claude",
  binary: "claude",
  host_only: false,
  installed: true,
  install_hint: "",
};

const custom: AgentInfo = {
  kind: "custom",
  name: "remote-helper",
  binary: "remote-helper",
  host_only: false,
  installed: true,
  install_hint: "Configured custom agent",
};

const uninstalledBuiltin: AgentInfo = {
  kind: "builtin",
  name: "uninstalled-builtin",
  binary: "uninstalled-builtin",
  host_only: false,
  installed: false,
  install_hint: "brew install x",
};

function renderAgentStep(overrides: {
  tool?: string;
  agents?: AgentInfo[];
  cockpitMasterEnabled?: boolean;
}) {
  const onChange = vi.fn();
  const utils = render(
    <AgentStep
      data={{ ...initialData, tool: overrides.tool ?? "claude" }}
      onChange={onChange}
      agents={overrides.agents ?? [builtin, custom]}
      profiles={[] as ProfileInfo[]}
      dockerAvailable={false}
      onApplyProfileDefaults={() => {}}
      cockpitMasterEnabled={overrides.cockpitMasterEnabled ?? true}
    />,
  );
  return { onChange, ...utils };
}

describe("AgentStep custom-agent selection (#1252)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows custom agents in the picker with a Custom badge and hides uninstalled built-ins", () => {
    const { getByRole, queryByRole, queryAllByText } = renderAgentStep({
      tool: "claude",
      agents: [builtin, custom, uninstalledBuiltin],
    });

    expect(getByRole("button", { name: /claude/ })).toBeTruthy();
    expect(getByRole("button", { name: /remote-helper/ })).toBeTruthy();
    expect(queryByRole("button", { name: "uninstalled-builtin", exact: true })).toBeNull();
    expect(queryAllByText("Custom").length).toBeGreaterThan(0);
  });

  it("hides the No agents installed warning when only a custom agent is configured", () => {
    const { queryByText } = renderAgentStep({
      tool: "remote-helper",
      agents: [custom],
    });
    expect(queryByText("No agents installed")).toBeNull();
  });

  it("renders the custom-agent substrate notice when the selected agent is kind=custom", () => {
    const { getByText } = renderAgentStep({
      tool: "remote-helper",
      agents: [builtin, custom],
    });
    expect(
      getByText(
        "Custom agents run in the terminal. Cockpit is available for built-in agents with ACP support.",
      ),
    ).toBeTruthy();
  });

  it("renders the ACP substrate notice when the selected agent is a built-in with ACP support", () => {
    const { getByText } = renderAgentStep({
      tool: "claude",
      agents: [builtin, custom],
    });
    expect(getByText(/Cockpit is enabled/)).toBeTruthy();
  });

  it("clicking an agent button calls onChange with the agent name", () => {
    const { onChange, getByRole } = renderAgentStep({
      tool: "claude",
      agents: [builtin, custom],
    });
    fireEvent.click(getByRole("button", { name: /remote-helper/ }));
    expect(onChange).toHaveBeenCalledWith("tool", "remote-helper");
  });
});

describe("ReviewStep agent row (#1252)", () => {
  function renderReviewStep(overrides: {
    tool: string;
    agents?: AgentInfo[];
  }) {
    return render(
      <ReviewStep
        data={{
          ...initialData,
          path: "/tmp/example",
          title: "session",
          tool: overrides.tool,
        }}
        onChange={() => {}}
        agents={overrides.agents ?? [builtin, custom]}
        isSubmitting={false}
        error={null}
        onSubmit={() => {}}
        onJumpTo={() => {}}
        steps={[{ id: "agent", label: "Agent" }] as Parameters<typeof ReviewStep>[0]["steps"]}
      />,
    );
  }

  it("renders just the agent name when the selected agent is built-in", () => {
    const { queryAllByText } = renderReviewStep({ tool: "claude" });
    // The "Custom" badge text should not appear in the review row.
    expect(queryAllByText("Custom").length).toBe(0);
  });

  it("renders a Custom badge in the agent row when the selected agent is custom", () => {
    const { queryAllByText } = renderReviewStep({ tool: "remote-helper" });
    expect(queryAllByText("Custom").length).toBeGreaterThan(0);
  });

  it("falls back to '(not set)' without crashing when data.tool does not match any agent", () => {
    const { getByText } = renderReviewStep({ tool: "" });
    expect(getByText("(not set)")).toBeTruthy();
  });
});
