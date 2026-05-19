// @vitest-environment jsdom
//
// Presentational contract test for TopBar. TopBar is a pure prop-driven
// component (it pulls no data on its own), so this suite renders it
// directly with the prop permutations we care about and asserts the
// surface badges/buttons match. The full mounted topbar is exercised
// end-to-end in web/tests/top-bar.spec.ts; that suite covers menu
// interaction but cannot exercise the dev-build badge without mocking
// `/api/about`, which is what this Vitest file does instead.
//
// Part of #1055 (DEV build badge so concurrently-running debug/release
// instances on ports 8081 / 8080 are visually distinguishable).

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";

import { TopBar } from "../TopBar";

afterEach(() => {
  cleanup();
});

function renderTopBar(overrides: { isDevBuild?: boolean; isOffline?: boolean } = {}) {
  return render(
    <TopBar
      activeWorkspace={undefined}
      activeSession={null}
      onToggleSidebar={vi.fn()}
      onOpenPalette={vi.fn()}
      onToggleDiff={vi.fn()}
      diffCollapsed={true}
      onOpenHelp={vi.fn()}
      onOpenAbout={vi.fn()}
      onLogout={vi.fn()}
      loginRequired={false}
      isOffline={overrides.isOffline ?? false}
      isDevBuild={overrides.isDevBuild ?? false}
      onGoDashboard={vi.fn()}
    />,
  );
}

describe("TopBar", () => {
  it("renders the DEV badge when isDevBuild=true", () => {
    const { getByLabelText, getByText } = renderTopBar({ isDevBuild: true });
    const badge = getByLabelText("Debug build");
    expect(badge).toBeTruthy();
    expect(getByText("DEV")).toBeTruthy();
  });

  it("does not render the DEV badge when isDevBuild=false", () => {
    const { queryByLabelText, queryByText } = renderTopBar({ isDevBuild: false });
    expect(queryByLabelText("Debug build")).toBeNull();
    expect(queryByText("DEV")).toBeNull();
  });

  it("renders the offline badge independent of the DEV badge", () => {
    const { getByText, getByLabelText } = renderTopBar({
      isDevBuild: true,
      isOffline: true,
    });
    expect(getByText("offline")).toBeTruthy();
    expect(getByLabelText("Debug build")).toBeTruthy();
  });
});
