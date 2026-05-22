// @vitest-environment jsdom
//
// Contract test for the UpdateSettings panel. Mirrors the SoundSettings
// canonical example (see SoundSettings.test.tsx) and is part of #1217.
// Updated for #1140: `update_check_mode` replaces `check_enabled` and the
// legacy `auto_update` toggle.

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
  it("changing update_check_mode emits updates.update_check_mode", () => {
    const { onSaveField, onUpdate, container } = mount({
      update_check_mode: "notify",
    });
    const select = container.querySelector("select") as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "off" } });
    expect(onSaveField).toHaveBeenCalledWith(
      "updates",
      "update_check_mode",
      "off",
    );
    expect(onUpdate).toHaveBeenCalledWith({
      updates: expect.objectContaining({ update_check_mode: "off" }),
    });
  });

  it("defaults the select to notify when update_check_mode is missing", () => {
    const { container } = mount({});
    const select = container.querySelector("select") as HTMLSelectElement;
    expect(select.value).toBe("notify");
  });

  it("auto mode selectable", () => {
    const { onSaveField, container } = mount({ update_check_mode: "notify" });
    const select = container.querySelector("select") as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "auto" } });
    expect(onSaveField).toHaveBeenCalledWith(
      "updates",
      "update_check_mode",
      "auto",
    );
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
    const toggle = container.querySelector(
      "button[role=switch]",
    ) as HTMLButtonElement;
    fireEvent.click(toggle);
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
      update_check_mode: "notify",
      check_interval_hours: 24,
      notify_in_cli: true,
      web_poll_interval_minutes: 60,
    });
    const select = container.querySelector("select") as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "off" } });
    expect(onUpdate).toHaveBeenCalledWith({
      updates: {
        update_check_mode: "off",
        check_interval_hours: 24,
        notify_in_cli: true,
        web_poll_interval_minutes: 60,
      },
    });
  });
});
