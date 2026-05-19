// @vitest-environment jsdom
//
// Contract test for the UpdateSettings panel. Mirrors the SoundSettings
// canonical example (see SoundSettings.test.tsx) and is part of #1217.

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/react";
import { UpdateSettings } from "../UpdateSettings";

function mount(initial: Record<string, unknown> = {}) {
  const onSaveField = vi.fn();
  const onUpdate = vi.fn();
  const { container } = render(
    <UpdateSettings
      settings={{ updates: initial }}
      onSaveField={onSaveField}
      onUpdate={onUpdate}
    />,
  );
  return { onSaveField, onUpdate, container };
}

function commitNumber(input: HTMLInputElement, value: string) {
  fireEvent.focus(input);
  fireEvent.change(input, { target: { value } });
  fireEvent.blur(input);
}

describe("UpdateSettings contract", () => {
  it("toggling check_enabled off emits updates.check_enabled=false", () => {
    const { onSaveField, onUpdate, container } = mount({ check_enabled: true });
    const toggles = container.querySelectorAll(
      "button[role=switch]",
    ) as NodeListOf<HTMLButtonElement>;
    fireEvent.click(toggles[0]);
    expect(onSaveField).toHaveBeenCalledWith("updates", "check_enabled", false);
    expect(onUpdate).toHaveBeenCalledWith({
      updates: expect.objectContaining({ check_enabled: false }),
    });
  });

  it("toggling auto_update on emits updates.auto_update=true", () => {
    const { onSaveField, container } = mount({ auto_update: false });
    const toggles = container.querySelectorAll(
      "button[role=switch]",
    ) as NodeListOf<HTMLButtonElement>;
    fireEvent.click(toggles[1]);
    expect(onSaveField).toHaveBeenCalledWith("updates", "auto_update", true);
  });

  it("check_interval_hours commits a positive value", () => {
    const { onSaveField, container } = mount({ check_interval_hours: 24 });
    const inputs = container.querySelectorAll(
      "input[type=number]",
    ) as NodeListOf<HTMLInputElement>;
    commitNumber(inputs[0], "12");
    expect(onSaveField).toHaveBeenCalledWith(
      "updates",
      "check_interval_hours",
      12,
    );
  });

  it("check_interval_hours clamps to a minimum of 1", () => {
    const { onSaveField, container } = mount({ check_interval_hours: 24 });
    const inputs = container.querySelectorAll(
      "input[type=number]",
    ) as NodeListOf<HTMLInputElement>;
    commitNumber(inputs[0], "0");
    expect(onSaveField).toHaveBeenCalledWith(
      "updates",
      "check_interval_hours",
      1,
    );
  });

  it("toggling notify_in_cli off emits updates.notify_in_cli=false", () => {
    const { onSaveField, container } = mount({ notify_in_cli: true });
    const toggles = container.querySelectorAll(
      "button[role=switch]",
    ) as NodeListOf<HTMLButtonElement>;
    fireEvent.click(toggles[2]);
    expect(onSaveField).toHaveBeenCalledWith(
      "updates",
      "notify_in_cli",
      false,
    );
  });

  it("web_poll_interval_minutes commits a value above the floor", () => {
    const { onSaveField, container } = mount({
      web_poll_interval_minutes: 60,
    });
    const inputs = container.querySelectorAll(
      "input[type=number]",
    ) as NodeListOf<HTMLInputElement>;
    commitNumber(inputs[1], "30");
    expect(onSaveField).toHaveBeenCalledWith(
      "updates",
      "web_poll_interval_minutes",
      30,
    );
  });

  it("web_poll_interval_minutes clamps to a minimum of 5", () => {
    const { onSaveField, container } = mount({
      web_poll_interval_minutes: 60,
    });
    const inputs = container.querySelectorAll(
      "input[type=number]",
    ) as NodeListOf<HTMLInputElement>;
    commitNumber(inputs[1], "1");
    expect(onSaveField).toHaveBeenCalledWith(
      "updates",
      "web_poll_interval_minutes",
      5,
    );
  });

  it("onUpdate merges patch into existing updates state", () => {
    const { onUpdate, container } = mount({
      check_enabled: true,
      auto_update: false,
      check_interval_hours: 24,
      notify_in_cli: true,
      web_poll_interval_minutes: 60,
    });
    const toggles = container.querySelectorAll(
      "button[role=switch]",
    ) as NodeListOf<HTMLButtonElement>;
    fireEvent.click(toggles[1]); // auto_update -> true
    expect(onUpdate).toHaveBeenCalledWith({
      updates: {
        check_enabled: true,
        auto_update: true,
        check_interval_hours: 24,
        notify_in_cli: true,
        web_poll_interval_minutes: 60,
      },
    });
  });
});
