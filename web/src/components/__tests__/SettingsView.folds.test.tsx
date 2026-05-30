// @vitest-environment jsdom
//
// Behavioral coverage for the Settings "Advanced" folds (#1515):
//   Story #2 - advanced cockpit knobs are hidden behind a default-collapsed
//              fold while high-level controls stay visible.
//   Story #4 - the fold collapses back to default when the user changes tabs
//              or switches profiles (component-local state, not persisted).
//
// The end-to-end persist-after-expand path (story #3) lives in live Playwright
// at web/tests/live/settings-advanced-fold.spec.ts.

import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { SettingsView } from "../SettingsView";

const PROFILES = [
  { name: "main", is_default: true },
  { name: "work", is_default: false },
];

vi.mock("../../lib/api", () => ({
  fetchProfiles: vi.fn(() => Promise.resolve(PROFILES)),
  fetchSettings: vi.fn(() =>
    Promise.resolve({ cockpit: {}, sandbox: {}, worktree: {} }),
  ),
  updateProfileSettings: vi.fn(() => Promise.resolve(true)),
  setCockpitMaster: vi.fn(() => Promise.resolve(true)),
  setDefaultProfile: vi.fn(() => Promise.resolve(true)),
  createProfile: vi.fn(() => Promise.resolve(true)),
  renameProfile: vi.fn(() => Promise.resolve(true)),
  deleteProfile: vi.fn(() => Promise.resolve(true)),
}));

const SERVER_ABOUT = {
  cockpit_master_enabled: true,
  cockpit_show_tool_durations: true,
  cockpit_queue_drain_mode: "combined" as const,
  cockpit_max_concurrent_resumes: 4,
};

function renderView(tab: string) {
  const onSelectTab = vi.fn();
  const utils = render(
    <SettingsView
      onClose={() => {}}
      tab={tab}
      onSelectTab={onSelectTab}
      serverAbout={SERVER_ABOUT as never}
      onServerAboutRefresh={() => {}}
    />,
  );
  return { ...utils, onSelectTab };
}

function expandAdvanced(container: HTMLElement) {
  const trigger = container.querySelector(
    "button[aria-expanded]",
  ) as HTMLButtonElement;
  expect(trigger).toBeTruthy();
  fireEvent.click(trigger);
}

// The profile picker is the only <select> carrying the "work" option.
function selectProfile(container: HTMLElement, name: string) {
  const select = Array.from(container.querySelectorAll("select")).find((s) =>
    Array.from(s.options).some((o) => o.value === name),
  ) as HTMLSelectElement;
  expect(select).toBeTruthy();
  fireEvent.change(select, { target: { value: name } });
}

describe("Settings Advanced fold", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("hides cockpit advanced knobs until the fold is expanded (#2)", async () => {
    const { container } = renderView("cockpit");

    // High-level controls are always visible.
    expect(screen.getByText("Cockpit master switch")).toBeTruthy();
    expect(screen.getByText("Show tool-call durations")).toBeTruthy();
    expect(screen.getByText("Queue drain mode")).toBeTruthy();

    // Advanced knobs are absent while collapsed.
    expect(screen.queryByText("Replay buffer bytes")).toBeNull();
    expect(screen.queryByText("Max concurrent resumes")).toBeNull();
    expect(screen.queryByText("Silent-orphan grace (s)")).toBeNull();

    expandAdvanced(container);

    expect(screen.getByText("Replay buffer bytes")).toBeTruthy();
    expect(screen.getByText("Max concurrent resumes")).toBeTruthy();
    expect(screen.getByText("Silent-orphan grace (s)")).toBeTruthy();
  });

  it("collapses the fold when switching tabs, with no cross-tab leak (#4)", async () => {
    const { container, rerender } = renderView("sandbox");
    await screen.findByText("Sandbox enabled by default");

    expandAdvanced(container);
    expect(screen.getByText("CPU limit")).toBeTruthy();

    // Switch to worktree: its Advanced fold starts collapsed (no leaked
    // open-state from the sandbox tab sharing the same root element).
    rerender(
      <SettingsView
        onClose={() => {}}
        tab="worktree"
        onSelectTab={() => {}}
        serverAbout={SERVER_ABOUT as never}
        onServerAboutRefresh={() => {}}
      />,
    );
    await screen.findByText("Worktrees enabled");
    expect(screen.queryByText("Bare repo path template")).toBeNull();

    // Back to sandbox: the fold reset to collapsed.
    rerender(
      <SettingsView
        onClose={() => {}}
        tab="sandbox"
        onSelectTab={() => {}}
        serverAbout={SERVER_ABOUT as never}
        onServerAboutRefresh={() => {}}
      />,
    );
    await screen.findByText("Sandbox enabled by default");
    expect(screen.queryByText("CPU limit")).toBeNull();
  });

  it("collapses the fold when switching profiles (#4)", async () => {
    const { container } = renderView("cockpit");
    await waitFor(() => expect(screen.getByText("Queue drain mode")).toBeTruthy());

    expandAdvanced(container);
    expect(screen.getByText("Replay buffer bytes")).toBeTruthy();

    selectProfile(container, "work");

    await waitFor(() =>
      expect(screen.queryByText("Replay buffer bytes")).toBeNull(),
    );
  });
});
