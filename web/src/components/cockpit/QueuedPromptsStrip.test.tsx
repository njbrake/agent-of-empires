// @vitest-environment jsdom
//
// Render coverage for the QueuedPromptsStrip clear-boundary divider
// (#1356). The strip lifts up a visual hint when the queued items will
// fire as separate POSTs because one of them is a clear-command alias.

import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { AgentProfileProvider } from "../../lib/agentProfileContext";
import type { QueuedPrompt } from "../../lib/cockpitTypes";
import { QueuedPromptsStrip } from "./CockpitView";

function mk(id: string, text: string): QueuedPrompt {
  return { id, text, queuedAt: "2026-05-21T00:00:00.000Z" };
}

function renderWithProfile(
  toolKey: string,
  queued: QueuedPrompt[],
) {
  return render(
    <AgentProfileProvider toolKey={toolKey}>
      <QueuedPromptsStrip
        queued={queued}
        onRemove={() => {}}
        onEdit={() => {}}
        onClear={() => {}}
      />
    </AgentProfileProvider>,
  );
}

describe("QueuedPromptsStrip clear-boundary divider (#1356)", () => {
  afterEach(() => {
    cleanup();
  });

  it("renders a divider above a queued /clear under the claude profile", () => {
    // Two-entry queue stays under the desktop visibleDefault=2 collapse
    // threshold so both rows render without an expand click.
    const { queryAllByTestId } = renderWithProfile("claude", [
      mk("a", "first"),
      mk("c", "/clear"),
    ]);
    expect(queryAllByTestId("queued-clear-boundary")).toHaveLength(1);
  });

  it("renders a divider below a queued /clear when it leads the visible queue", () => {
    const { queryAllByTestId } = renderWithProfile("claude", [
      mk("c", "/clear"),
      mk("b", "second"),
    ]);
    expect(queryAllByTestId("queued-clear-boundary")).toHaveLength(1);
  });

  it("renders no divider when the queue contains no clear-command aliases", () => {
    const { queryAllByTestId } = renderWithProfile("claude", [
      mk("a", "first"),
      mk("b", "second"),
    ]);
    expect(queryAllByTestId("queued-clear-boundary")).toHaveLength(0);
  });

  it("renders nothing when the queue is empty", () => {
    const { container } = renderWithProfile("claude", []);
    expect(container.firstChild).toBeNull();
  });

  it("does not render a divider for an agent profile without clear aliases (gemini)", () => {
    const { queryAllByTestId } = renderWithProfile("gemini", [
      mk("a", "first"),
      mk("c", "/clear"),
    ]);
    // gemini's clearAliases are empty; even with `/clear` text in the
    // queue, the strip should not show a boundary because the agent
    // does not honour `/clear`.
    expect(queryAllByTestId("queued-clear-boundary")).toHaveLength(0);
  });

  it("treats `/new` as a boundary under the codex profile", () => {
    const { queryAllByTestId } = renderWithProfile("codex", [
      mk("a", "first"),
      mk("n", "/new"),
    ]);
    expect(queryAllByTestId("queued-clear-boundary")).toHaveLength(1);
  });

  it("treats a `/clear` invocation with trailing args as a boundary", () => {
    const { queryAllByTestId } = renderWithProfile("claude", [
      mk("a", "first"),
      mk("c", "/clear --hard"),
    ]);
    expect(queryAllByTestId("queued-clear-boundary")).toHaveLength(1);
  });

  it("renders the Clear all button when the queue has more than one entry", () => {
    const { getByRole } = renderWithProfile("claude", [
      mk("a", "first"),
      mk("b", "second"),
    ]);
    expect(getByRole("button", { name: /clear all/i })).toBeTruthy();
  });

  it("omits the Clear all button when the queue has exactly one entry", () => {
    const { queryByRole } = renderWithProfile("claude", [mk("a", "only")]);
    expect(queryByRole("button", { name: /clear all/i })).toBeNull();
  });
});
