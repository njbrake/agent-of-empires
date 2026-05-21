// @vitest-environment jsdom
//
// Switch-substrate action button + confirm dialog. The live spec
// covers the round-trip; this spec pins the button states (label
// flip by current substrate, ACP-disabled hint, offline disable),
// the confirm dialog routing, and the POST endpoint shape.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  waitFor,
} from "@testing-library/react";

import { SwitchSubstrateAction } from "./SwitchSubstrateAction";

let mockOffline = false;
vi.mock("../../lib/connectionState", () => ({
  useServerDown: () => mockOffline,
  OFFLINE_TITLE: "Disconnected",
}));

function mockOkFetch(): ReturnType<typeof vi.fn> {
  const fn = vi.fn().mockResolvedValue({
    ok: true,
    status: 200,
    text: async () => "",
  });
  vi.stubGlobal("fetch", fn);
  return fn;
}

function mockBadFetch(body = "boom", status = 500): ReturnType<typeof vi.fn> {
  const fn = vi.fn().mockResolvedValue({
    ok: false,
    status,
    text: async () => body,
  });
  vi.stubGlobal("fetch", fn);
  return fn;
}

beforeEach(() => {
  mockOffline = false;
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("SwitchSubstrateAction trigger", () => {
  it("labels the icon as 'Switch to cockpit mode' when current substrate is terminal", () => {
    render(
      <SwitchSubstrateAction sessionId="s-1" cockpitMode={false} />,
    );
    const btns = document.querySelectorAll(
      "button[aria-label='Switch to cockpit mode']",
    );
    expect(btns.length).toBeGreaterThan(0);
  });

  it("labels the icon as 'Switch to terminal mode' when current substrate is cockpit", () => {
    render(<SwitchSubstrateAction sessionId="s-1" cockpitMode={true} />);
    const btns = document.querySelectorAll(
      "button[aria-label='Switch to terminal mode']",
    );
    expect(btns.length).toBeGreaterThan(0);
  });

  it("renders 'Switch to terminal' text in button variant when cockpitMode is true", () => {
    const { getByText } = render(
      <SwitchSubstrateAction sessionId="s-1" cockpitMode={true} variant="button" />,
    );
    expect(getByText("Switch to terminal")).toBeTruthy();
  });

  it("is disabled when the target is cockpit and the agent is not ACP-capable", () => {
    const { getByLabelText } = render(
      <SwitchSubstrateAction
        sessionId="s-1"
        cockpitMode={false}
        acpCapable={false}
      />,
    );
    const btn = getByLabelText("Switch to cockpit mode") as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
    expect(btn.title).toMatch(/no ACP adapter/i);
  });

  it("is disabled when the server is offline", () => {
    mockOffline = true;
    const { getByLabelText } = render(
      <SwitchSubstrateAction sessionId="s-1" cockpitMode={false} />,
    );
    const btn = getByLabelText("Switch to cockpit mode") as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
    expect(btn.title).toMatch(/Disconnected/i);
  });
});

describe("SwitchSubstrateAction confirm dialog", () => {
  it("opens the confirm dialog on trigger click", () => {
    mockOkFetch();
    const { getByLabelText, getByRole } = render(
      <SwitchSubstrateAction sessionId="s-1" cockpitMode={false} />,
    );
    fireEvent.click(getByLabelText("Switch to cockpit mode"));
    expect(getByRole("dialog")).toBeTruthy();
  });

  it("cancel closes the dialog without firing fetch", () => {
    const fetchFn = mockOkFetch();
    const { getByLabelText, getByText, queryByRole } = render(
      <SwitchSubstrateAction sessionId="s-1" cockpitMode={false} />,
    );
    fireEvent.click(getByLabelText("Switch to cockpit mode"));
    fireEvent.click(getByText("Cancel"));
    expect(queryByRole("dialog")).toBeNull();
    expect(fetchFn).not.toHaveBeenCalled();
  });

  it("escape closes the dialog", () => {
    mockOkFetch();
    const { getByLabelText, queryByRole } = render(
      <SwitchSubstrateAction sessionId="s-1" cockpitMode={false} />,
    );
    fireEvent.click(getByLabelText("Switch to cockpit mode"));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(queryByRole("dialog")).toBeNull();
  });

  it("POSTs to /cockpit/enable when switching FROM terminal TO cockpit", async () => {
    const fetchFn = mockOkFetch();
    const { getByLabelText, getByText } = render(
      <SwitchSubstrateAction sessionId="s-1" cockpitMode={false} />,
    );
    fireEvent.click(getByLabelText("Switch to cockpit mode"));
    fireEvent.click(getByText("Switch"));
    await waitFor(() => expect(fetchFn).toHaveBeenCalledTimes(1));
    expect(fetchFn.mock.calls[0]?.[0]).toBe(
      "/api/sessions/s-1/cockpit/enable",
    );
    expect(fetchFn.mock.calls[0]?.[1]).toMatchObject({ method: "POST" });
  });

  it("POSTs to /cockpit/disable when switching FROM cockpit TO terminal", async () => {
    const fetchFn = mockOkFetch();
    const { getByLabelText, getByText } = render(
      <SwitchSubstrateAction sessionId="s-1" cockpitMode={true} />,
    );
    fireEvent.click(getByLabelText("Switch to terminal mode"));
    fireEvent.click(getByText("Switch"));
    await waitFor(() => expect(fetchFn).toHaveBeenCalledTimes(1));
    expect(fetchFn.mock.calls[0]?.[0]).toBe(
      "/api/sessions/s-1/cockpit/disable",
    );
  });

  it("URL-encodes the session id in the endpoint", async () => {
    const fetchFn = mockOkFetch();
    const { getByLabelText, getByText } = render(
      <SwitchSubstrateAction sessionId="weird/id" cockpitMode={false} />,
    );
    fireEvent.click(getByLabelText("Switch to cockpit mode"));
    fireEvent.click(getByText("Switch"));
    await waitFor(() => expect(fetchFn).toHaveBeenCalled());
    expect(fetchFn.mock.calls[0]?.[0]).toBe(
      "/api/sessions/weird%2Fid/cockpit/enable",
    );
  });

  it("surfaces a server error response in the dialog", async () => {
    mockBadFetch("session not found", 404);
    const { getByLabelText, getByText, findByText, queryByRole } = render(
      <SwitchSubstrateAction sessionId="s-1" cockpitMode={false} />,
    );
    fireEvent.click(getByLabelText("Switch to cockpit mode"));
    fireEvent.click(getByText("Switch"));
    await findByText(/session not found/i);
    // Dialog stays open on error so the user can retry.
    expect(queryByRole("dialog")).not.toBeNull();
  });

  it("falls back to 'HTTP <status>' when the error body is empty", async () => {
    mockBadFetch("", 500);
    const { getByLabelText, getByText, findByText } = render(
      <SwitchSubstrateAction sessionId="s-1" cockpitMode={false} />,
    );
    fireEvent.click(getByLabelText("Switch to cockpit mode"));
    fireEvent.click(getByText("Switch"));
    await findByText(/HTTP 500/);
  });

  it("surfaces network rejection as the dialog error", async () => {
    const fn = vi.fn().mockRejectedValue(new Error("offline"));
    vi.stubGlobal("fetch", fn);
    const { getByLabelText, getByText, findByText } = render(
      <SwitchSubstrateAction sessionId="s-1" cockpitMode={false} />,
    );
    fireEvent.click(getByLabelText("Switch to cockpit mode"));
    fireEvent.click(getByText("Switch"));
    await findByText(/offline/);
  });
});
