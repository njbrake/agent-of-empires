// @vitest-environment jsdom
//
// Covers PairedShellPane / PairedTerminal render branches: the loading
// placeholder, the connected/reconnecting/disconnected banners, mobile
// chrome, the host/container shell switch, and the fullViewport keyboard
// padding. The live PTY path is exercised by the Playwright suites; this
// drives the conditional JSX deterministically with a mocked useTerminal.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

import type { SessionResponse } from "../../lib/types";

const ensureTerminal = vi.fn();
const manualReconnect = vi.fn();

const mockState = vi.hoisted(() => ({
  current: {
    connected: true,
    reconnecting: false,
    retryCount: 0,
    isInScrollback: false,
  },
}));
const mockKeyboard = vi.hoisted(() => ({
  current: {
    isMobile: false,
    keyboardOpen: false,
    keyboardHeight: 0,
    reservedKeyboardHeight: 0,
  },
}));

vi.mock("../../lib/api", () => ({
  ensureSession: vi.fn(),
  ensureTerminal: (id: string, container: boolean) => ensureTerminal(id, container),
}));

vi.mock("../../hooks/useTerminal", () => ({
  useTerminal: () => ({
    containerRef: { current: null },
    termRef: { current: null },
    state: mockState.current,
    manualReconnect,
    sendData: vi.fn(),
    activate: vi.fn(),
    exitScrollback: vi.fn(),
    ctrlActiveRef: { current: false },
    clearCtrlRef: { current: null },
    maxRetries: 7,
  }),
}));

vi.mock("../../hooks/useMobileKeyboard", () => ({
  useMobileKeyboard: () => mockKeyboard.current,
}));

vi.mock("../MobileTerminalToolbar", () => ({
  MobileTerminalToolbar: () => <div data-testid="mobile-toolbar" />,
}));
vi.mock("../KeyboardFab", () => ({
  KeyboardFab: () => <button data-testid="keyboard-fab" />,
}));
vi.mock("../BackToLiveButton", () => ({
  BackToLiveButton: () => <button data-testid="back-to-live" />,
}));

import { PairedShellPane } from "../PairedTerminal";

function session(overrides: Partial<SessionResponse> = {}): SessionResponse {
  return {
    id: "sess-1",
    title: "t",
    project_path: "/tmp/t",
    group_path: "/tmp",
    tool: "claude",
    status: "Running",
    yolo_mode: false,
    created_at: new Date().toISOString(),
    last_accessed_at: null,
    last_error: null,
    branch: null,
    main_repo_path: null,
    is_sandboxed: false,
    has_terminal: true,
    profile: "default",
    workspace_repos: [],
    ...overrides,
  } as SessionResponse;
}

beforeEach(() => {
  ensureTerminal.mockResolvedValue(true);
  mockState.current = {
    connected: true,
    reconnecting: false,
    retryCount: 0,
    isInScrollback: false,
  };
  mockKeyboard.current = {
    isMobile: false,
    keyboardOpen: false,
    keyboardHeight: 0,
    reservedKeyboardHeight: 0,
  };
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("PairedShellPane", () => {
  it("shows the placeholder while ensureTerminal is pending", () => {
    ensureTerminal.mockReturnValue(new Promise(() => {}));
    render(<PairedShellPane session={session()} sessionId="sess-1" />);
    expect(screen.getByText(/Starting terminal/i)).toBeDefined();
  });

  it("renders 'Select a session' when sessionId is null", () => {
    render(<PairedShellPane session={null} sessionId={null} />);
    expect(screen.getByText(/Select a session/i)).toBeDefined();
  });

  it("renders the terminal surface once ready", async () => {
    render(<PairedShellPane session={session()} sessionId="sess-1" />);
    await waitFor(() =>
      expect(document.querySelector('[data-term="paired"]')).not.toBeNull(),
    );
  });

  it("renders mobile chrome and scrollback affordance", async () => {
    mockKeyboard.current.isMobile = true;
    mockState.current.isInScrollback = true;
    render(<PairedShellPane session={session()} sessionId="sess-1" />);
    await screen.findByTestId("keyboard-fab");
    expect(screen.getByTestId("mobile-toolbar")).toBeDefined();
    expect(screen.getByTestId("back-to-live")).toBeDefined();
  });

  it("shows the reconnecting banner", async () => {
    mockState.current = {
      connected: false,
      reconnecting: true,
      retryCount: 2,
      isInScrollback: false,
    };
    render(<PairedShellPane session={session()} sessionId="sess-1" />);
    await screen.findByText(/Reconnecting/i);
  });

  it("shows the disconnected banner with a working Retry", async () => {
    mockState.current = {
      connected: false,
      reconnecting: false,
      retryCount: 7,
      isInScrollback: false,
    };
    render(<PairedShellPane session={session()} sessionId="sess-1" />);
    const retry = await screen.findByRole("button", { name: /Retry/i });
    fireEvent.click(retry);
    expect(manualReconnect).toHaveBeenCalled();
  });

  it("offers the Container switch for sandboxed sessions and re-ensures on switch", async () => {
    render(
      <PairedShellPane
        session={session({ is_sandboxed: true })}
        sessionId="sess-1"
      />,
    );
    await waitFor(() =>
      expect(document.querySelector('[data-term="paired"]')).not.toBeNull(),
    );
    fireEvent.click(screen.getByRole("button", { name: /^Container$/ }));
    await waitFor(() => expect(ensureTerminal).toHaveBeenCalledWith("sess-1", true));
  });

  it("reserves keyboard padding in fullViewport mode", async () => {
    mockKeyboard.current.reservedKeyboardHeight = 320;
    render(
      <PairedShellPane session={session()} sessionId="sess-1" fullViewport />,
    );
    await waitFor(() =>
      expect(document.querySelector('[data-term="paired"]')).not.toBeNull(),
    );
    const root = document.querySelector('[data-term="paired"]')
      ?.parentElement as HTMLElement;
    expect(root.style.paddingBottom).toBe("320px");
  });
});
