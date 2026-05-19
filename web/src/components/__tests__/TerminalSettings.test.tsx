// @vitest-environment jsdom
//
// Contract test for the TerminalSettings panel. Unlike the panels under
// settings/, this one persists through useWebSettings + localStorage
// (key `aoe-web-settings`) rather than PATCH /api/settings. The contract
// here is the JSON shape written to that key. Part of #1217.

import { beforeEach, describe, expect, it } from "vitest";
import { fireEvent, render } from "@testing-library/react";
import { TerminalSettings } from "../TerminalSettings";

const KEY = "aoe-web-settings";

function readStored(): Record<string, unknown> {
  const raw = window.localStorage.getItem(KEY);
  return raw ? (JSON.parse(raw) as Record<string, unknown>) : {};
}

beforeEach(() => {
  window.localStorage.clear();
});

describe("TerminalSettings localStorage contract", () => {
  it("mobile font slider writes mobileFontSize into aoe-web-settings", () => {
    const { container } = render(<TerminalSettings />);
    const slider = container.querySelectorAll(
      "input[type=range]",
    )[0] as HTMLInputElement;
    fireEvent.change(slider, { target: { value: "10" } });
    expect(readStored().mobileFontSize).toBe(10);
  });

  it("mobile font select writes mobileFontSize", () => {
    const { container } = render(<TerminalSettings />);
    const select = container.querySelectorAll(
      "select",
    )[0] as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "16" } });
    expect(readStored().mobileFontSize).toBe(16);
  });

  it("desktop font slider writes desktopFontSize", () => {
    const { container } = render(<TerminalSettings />);
    const slider = container.querySelectorAll(
      "input[type=range]",
    )[1] as HTMLInputElement;
    fireEvent.change(slider, { target: { value: "18" } });
    expect(readStored().desktopFontSize).toBe(18);
  });

  it("desktop font select writes desktopFontSize", () => {
    const { container } = render(<TerminalSettings />);
    const select = container.querySelectorAll(
      "select",
    )[1] as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "20" } });
    expect(readStored().desktopFontSize).toBe(20);
  });

  it("autoOpenKeyboard checkbox writes the boolean flag", () => {
    const { container } = render(<TerminalSettings />);
    const checkbox = container.querySelector(
      "input[type=checkbox]",
    ) as HTMLInputElement;
    fireEvent.click(checkbox);
    expect(readStored().autoOpenKeyboard).toBe(false);
  });

  it("preserves unrelated keys when persisting an update", () => {
    window.localStorage.setItem(
      KEY,
      JSON.stringify({
        mobileFontSize: 8,
        desktopFontSize: 14,
        autoOpenKeyboard: true,
        diffViewMode: "tree",
        collapsedDiffDirs: ["a/b"],
      }),
    );
    const { container } = render(<TerminalSettings />);
    const slider = container.querySelectorAll(
      "input[type=range]",
    )[0] as HTMLInputElement;
    fireEvent.change(slider, { target: { value: "12" } });
    const stored = readStored();
    expect(stored).toMatchObject({
      mobileFontSize: 12,
      desktopFontSize: 14,
      autoOpenKeyboard: true,
      diffViewMode: "tree",
      collapsedDiffDirs: ["a/b"],
    });
  });

  it("reflects the stored value on initial mount", () => {
    window.localStorage.setItem(
      KEY,
      JSON.stringify({
        mobileFontSize: 22,
        desktopFontSize: 16,
        autoOpenKeyboard: false,
      }),
    );
    const { container } = render(<TerminalSettings />);
    const mobileSelect = container.querySelectorAll(
      "select",
    )[0] as HTMLSelectElement;
    const desktopSelect = container.querySelectorAll(
      "select",
    )[1] as HTMLSelectElement;
    const checkbox = container.querySelector(
      "input[type=checkbox]",
    ) as HTMLInputElement;
    expect(mobileSelect.value).toBe("22");
    expect(desktopSelect.value).toBe("16");
    expect(checkbox.checked).toBe(false);
  });
});
