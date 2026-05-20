// @vitest-environment jsdom
//
// Modal-side contract for the rate-limit recovery flow (#1281 / #1282).
// The component fans out to three API helpers in lib/api; the test
// mocks them so each assertion pins one slice of behaviour:
//   - confirm fires switchCockpitAgent then fetchContextPrimer, in
//     that order, then onPrefill with the framed handoff text;
//   - cancel does NOT touch switchCockpitAgent;
//   - Escape closes the modal (and likewise leaves switch untouched);
//   - the recap and unprocessed_prompt slots show up in the prefill in
//     the expected positions.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  waitFor,
} from "@testing-library/react";

import { RateLimitRecoveryModal } from "./RateLimitRecoveryModal";

vi.mock("../../lib/api", () => ({
  fetchCockpitAgents: vi.fn(),
  switchCockpitAgent: vi.fn(),
  fetchContextPrimer: vi.fn(),
}));

import {
  fetchCockpitAgents,
  fetchContextPrimer,
  switchCockpitAgent,
} from "../../lib/api";

const mockFetchAgents = vi.mocked(fetchCockpitAgents);
const mockSwitch = vi.mocked(switchCockpitAgent);
const mockPrimer = vi.mocked(fetchContextPrimer);

beforeEach(() => {
  vi.clearAllMocks();
  mockFetchAgents.mockResolvedValue([
    { name: "claude", description: "Claude (Sonnet)", command: "claude-agent-acp" },
    { name: "codex", description: "OpenAI Codex", command: "codex-acp" },
    { name: "opencode", description: "OpenCode", command: "opencode-acp" },
  ]);
  mockSwitch.mockResolvedValue({
    session_id: "s-1",
    agent: "codex",
    before_seq: 41,
    switch_seq: 42,
    status: "switched",
  });
  mockPrimer.mockResolvedValue({
    primer: "user: hi\nagent: hello",
    included_event_count: 2,
    included_turn_count: 1,
    truncated: false,
    max_chars: 4_000,
    unprocessed_prompt: "deploy the thing",
  });
});

afterEach(() => {
  cleanup();
});

function mount(props?: Partial<React.ComponentProps<typeof RateLimitRecoveryModal>>) {
  const onClose = vi.fn();
  const onPrefill = vi.fn();
  const utils = render(
    <RateLimitRecoveryModal
      open
      sessionId="s-1"
      currentAgent="claude"
      onClose={onClose}
      onPrefill={onPrefill}
      {...props}
    />,
  );
  return { onClose, onPrefill, ...utils };
}

describe("RateLimitRecoveryModal", () => {
  it("filters out the current agent and preselects codex", async () => {
    const { container, findByText } = mount();
    await findByText(/Continue in codex/);
    const radios = Array.from(
      container.querySelectorAll<HTMLInputElement>(
        "input[name=cockpit-agent-target]",
      ),
    );
    const values = radios.map((r) => r.value);
    expect(values).toEqual(expect.arrayContaining(["codex", "opencode"]));
    expect(values).not.toContain("claude");
    const checked = radios.find((r) => r.checked);
    expect(checked?.value).toBe("codex");
  });

  it("falls back to the first remaining agent when codex isn't installed", async () => {
    mockFetchAgents.mockResolvedValue([
      { name: "claude", description: "Claude", command: "claude-agent-acp" },
      { name: "opencode", description: "OpenCode", command: "opencode-acp" },
    ]);
    const { findByText } = mount();
    await findByText(/Continue in opencode/);
  });

  it("hands off via switchCockpitAgent + fetchContextPrimer and prefills", async () => {
    const { findByText, onPrefill, onClose } = mount();
    const confirm = await findByText(/Continue in codex/);
    fireEvent.click(confirm);
    await waitFor(() => expect(mockSwitch).toHaveBeenCalledTimes(1));
    expect(mockSwitch).toHaveBeenCalledWith("s-1", "codex");
    await waitFor(() => expect(mockPrimer).toHaveBeenCalledTimes(1));
    // Primer must be invoked with before_seq from the switch response
    // (41), not switch_seq, so the recap excludes the AgentSwitched
    // event itself.
    expect(mockPrimer.mock.calls[0]?.[1]).toBe(41);

    await waitFor(() => expect(onPrefill).toHaveBeenCalledTimes(1));
    const prefilled = onPrefill.mock.calls[0]?.[0] as string;
    expect(prefilled).toContain("CONTEXT HANDOFF");
    expect(prefilled).toContain("claude");
    expect(prefilled).toContain("codex");
    expect(prefilled).toContain("user: hi");
    expect(prefilled).toContain("deploy the thing");
    expect(prefilled.indexOf("user: hi")).toBeLessThan(
      prefilled.indexOf("deploy the thing"),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not call switchCockpitAgent when the user cancels", async () => {
    const { findByText, onClose } = mount();
    await findByText(/Continue in codex/);
    fireEvent.click(await findByText("Cancel"));
    expect(mockSwitch).not.toHaveBeenCalled();
    expect(mockPrimer).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape without dispatching a switch", async () => {
    const { findByText, onClose } = mount();
    await findByText(/Continue in codex/);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(mockSwitch).not.toHaveBeenCalled();
  });

  it("surfaces a server error and keeps the modal open", async () => {
    mockSwitch.mockRejectedValue(new Error("boom"));
    const { findByText, onPrefill, onClose } = mount();
    fireEvent.click(await findByText(/Continue in codex/));
    const alert = await findByText(/boom/);
    expect(alert.textContent).toMatch(/boom/);
    expect(onPrefill).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("surfaces fetchCockpitAgents rejection in the modal error slot", async () => {
    mockFetchAgents.mockRejectedValue(new Error("agents fetch broke"));
    const { findByText, onPrefill } = mount();
    const alert = await findByText(/agents fetch broke/);
    expect(alert.textContent).toMatch(/agents fetch broke/);
    expect(mockSwitch).not.toHaveBeenCalled();
    expect(onPrefill).not.toHaveBeenCalled();
  });

  it("surfaces a generic message when switchCockpitAgent returns null", async () => {
    // The api helper returns null on 4xx/5xx without throwing (fetchJson
    // semantics). Modal must not crash and must show a clear message.
    mockSwitch.mockResolvedValue(null);
    const { findByText, onPrefill, onClose } = mount();
    fireEvent.click(await findByText(/Continue in codex/));
    const alert = await findByText(/server returned no response/i);
    expect(alert.textContent).toMatch(/server returned no response/i);
    expect(mockPrimer).not.toHaveBeenCalled();
    expect(onPrefill).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("clicking a non-preselected radio updates the confirm-button target", async () => {
    const { container, findByText } = mount();
    await findByText(/Continue in codex/);
    const opencodeRadio = container.querySelector<HTMLInputElement>(
      "input[name=cockpit-agent-target][value=opencode]",
    );
    expect(opencodeRadio).not.toBeNull();
    fireEvent.click(opencodeRadio!);
    await findByText(/Continue in opencode/);
  });

  it("renders an install hint when no alternative agents are registered", async () => {
    mockFetchAgents.mockResolvedValue([
      { name: "claude", description: "claude", command: "claude-agent-acp" },
    ]);
    const { findByText } = mount();
    await findByText(/No alternative cockpit agents are registered/i);
  });
});
