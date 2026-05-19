// @vitest-environment jsdom
//
// Contract test for the SoundSettings panel: every control invokes the
// parent's onSaveField and onUpdate callbacks with the exact shape the
// server expects under `sound.*`. This is the canonical example of the
// "Vitest + RTL contract test for request-payload permutations" pattern
// described in docs/development/playwright.md and AGENTS.md.
//
// Follow-up issue #1217 extends this pattern to every other settings panel
// (and is the right place to add proper htmlFor/id wiring on FormFields so
// these tests can switch to getByLabelText queries instead of containers).

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/react";
import { SoundSettings } from "../SoundSettings";

function mount(initial: Record<string, unknown> = {}) {
  const onSaveField = vi.fn();
  const onUpdate = vi.fn();
  const { container } = render(
    <SoundSettings
      settings={{ sound: initial }}
      onSaveField={onSaveField}
      onUpdate={onUpdate}
    />,
  );
  return { onSaveField, onUpdate, container };
}

function commitText(input: HTMLInputElement, value: string) {
  fireEvent.focus(input);
  fireEvent.change(input, { target: { value } });
  fireEvent.blur(input);
}

describe("SoundSettings contract", () => {
  it("toggling enabled emits enabled=true through both callbacks", () => {
    const { onSaveField, onUpdate, container } = mount({ enabled: false });
    const toggle = container.querySelector(
      "button[role=switch]",
    ) as HTMLButtonElement;
    fireEvent.click(toggle);
    expect(onSaveField).toHaveBeenCalledWith("sound", "enabled", true);
    expect(onUpdate).toHaveBeenCalledWith({ sound: { enabled: true } });
  });

  it("toggling enabled off emits enabled=false", () => {
    const { onSaveField, onUpdate, container } = mount({ enabled: true });
    const toggle = container.querySelector(
      "button[role=switch]",
    ) as HTMLButtonElement;
    fireEvent.click(toggle);
    expect(onSaveField).toHaveBeenCalledWith("sound", "enabled", false);
    expect(onUpdate).toHaveBeenCalledWith({
      sound: expect.objectContaining({ enabled: false }),
    });
  });

  describe("when enabled", () => {
    it("mode 'random' patches sound.mode='random'", () => {
      const { onSaveField, container } = mount({
        enabled: true,
        mode: { specific: "" },
      });
      const select = container.querySelector("select") as HTMLSelectElement;
      fireEvent.change(select, { target: { value: "random" } });
      expect(onSaveField).toHaveBeenCalledWith("sound", "mode", "random");
    });

    it("mode 'specific' patches sound.mode={specific: ''}", () => {
      const { onSaveField, container } = mount({
        enabled: true,
        mode: "random",
      });
      const select = container.querySelector("select") as HTMLSelectElement;
      fireEvent.change(select, { target: { value: "specific" } });
      expect(onSaveField).toHaveBeenCalledWith("sound", "mode", {
        specific: "",
      });
    });

    it("volume slider patches sound.volume", () => {
      const { onSaveField, container } = mount({ enabled: true, volume: 1.0 });
      const slider = container.querySelector(
        "input[type=range]",
      ) as HTMLInputElement;
      fireEvent.change(slider, { target: { value: "0.5" } });
      expect(onSaveField).toHaveBeenCalledWith("sound", "volume", 0.5);
    });

    it("on_start text field commits non-empty value on blur", () => {
      const { onSaveField, container } = mount({ enabled: true });
      const inputs = container.querySelectorAll(
        "input[type=text]",
      ) as NodeListOf<HTMLInputElement>;
      // SoundSettings renders the three text fields in order:
      // on_start (0), on_waiting (1), on_error (2).
      commitText(inputs[0], "startup.wav");
      expect(onSaveField).toHaveBeenCalledWith(
        "sound",
        "on_start",
        "startup.wav",
      );
    });

    it("on_start commits null when cleared", () => {
      const { onSaveField, container } = mount({
        enabled: true,
        on_start: "x.wav",
      });
      const input = container.querySelectorAll(
        "input[type=text]",
      )[0] as HTMLInputElement;
      commitText(input, "");
      expect(onSaveField).toHaveBeenCalledWith("sound", "on_start", null);
    });

    it("on_waiting commits non-empty on blur", () => {
      const { onSaveField, container } = mount({ enabled: true });
      const input = container.querySelectorAll(
        "input[type=text]",
      )[1] as HTMLInputElement;
      commitText(input, "waiting.wav");
      expect(onSaveField).toHaveBeenCalledWith(
        "sound",
        "on_waiting",
        "waiting.wav",
      );
    });

    it("on_error commits non-empty on blur", () => {
      const { onSaveField, container } = mount({ enabled: true });
      const input = container.querySelectorAll(
        "input[type=text]",
      )[2] as HTMLInputElement;
      commitText(input, "error.wav");
      expect(onSaveField).toHaveBeenCalledWith(
        "sound",
        "on_error",
        "error.wav",
      );
    });
  });

  it("hides mode/volume/file fields when disabled", () => {
    const { container } = mount({ enabled: false });
    expect(container.querySelector("select")).toBeNull();
    expect(container.querySelector("input[type=range]")).toBeNull();
    expect(container.querySelector("input[type=text]")).toBeNull();
  });

  it("onUpdate merges patch into existing sound state", () => {
    const { onUpdate, container } = mount({
      enabled: true,
      volume: 1.0,
      on_start: "a.wav",
    });
    const slider = container.querySelector(
      "input[type=range]",
    ) as HTMLInputElement;
    fireEvent.change(slider, { target: { value: "0.7" } });
    expect(onUpdate).toHaveBeenCalledWith({
      sound: {
        enabled: true,
        volume: 0.7,
        on_start: "a.wav",
      },
    });
  });
});
