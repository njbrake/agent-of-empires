// @vitest-environment jsdom
//
// Contract test for the ThemeSettings panel. Live persistence is already
// covered by tests/live/settings-persistence-theme.spec.ts; this file
// drills into the callback payload shape for both controls (theme name +
// color mode). Part of #1217.

import { afterEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/react";

vi.mock("../../../lib/api", () => ({
  fetchThemes: vi.fn(() =>
    Promise.resolve(["default", "modus-vivendi", "empire"]),
  ),
}));

const dispatchSpy = vi.fn();
vi.mock("../../../hooks/useResolvedTheme", () => ({
  dispatchThemePickerChanged: (name?: string) => dispatchSpy(name),
}));

import { ThemeSettings } from "../ThemeSettings";

function mount(initial: Record<string, unknown> = {}) {
  const onSaveField = vi.fn();
  const onUpdate = vi.fn();
  const { container } = render(
    <ThemeSettings
      settings={{ theme: initial }}
      onSaveField={onSaveField}
      onUpdate={onUpdate}
    />,
  );
  return { onSaveField, onUpdate, container };
}

afterEach(() => {
  dispatchSpy.mockClear();
});

describe("ThemeSettings contract", () => {
  it("theme name select emits theme.name and dispatches a picker event", async () => {
    const { onSaveField, onUpdate, container } = mount({
      name: "default",
      color_mode: "truecolor",
    });
    // Wait for fetchThemes to resolve and the options to render.
    await waitFor(() => {
      const opts = container.querySelectorAll("select")[0].querySelectorAll(
        "option",
      );
      expect(opts.length).toBeGreaterThan(1);
    });
    const themeSelect = container.querySelectorAll("select")[0];
    fireEvent.change(themeSelect, { target: { value: "modus-vivendi" } });
    expect(onSaveField).toHaveBeenCalledWith("theme", "name", "modus-vivendi");
    expect(onUpdate).toHaveBeenCalledWith({
      theme: { name: "modus-vivendi", color_mode: "truecolor" },
    });
    expect(dispatchSpy).toHaveBeenCalledWith("modus-vivendi");
  });

  it("color_mode select emits theme.color_mode without firing the picker event", () => {
    const { onSaveField, onUpdate, container } = mount({
      name: "default",
      color_mode: "truecolor",
    });
    const colorSelect = container.querySelectorAll("select")[1];
    fireEvent.change(colorSelect, { target: { value: "palette" } });
    expect(onSaveField).toHaveBeenCalledWith("theme", "color_mode", "palette");
    expect(onUpdate).toHaveBeenCalledWith({
      theme: { name: "default", color_mode: "palette" },
    });
    // Picker event is only for name changes.
    expect(dispatchSpy).not.toHaveBeenCalled();
  });

  it("color_mode defaults to 'truecolor' when absent", () => {
    const { container } = mount({ name: "default" });
    const colorSelect = container.querySelectorAll(
      "select",
    )[1] as HTMLSelectElement;
    expect(colorSelect.value).toBe("truecolor");
  });

  it("each color_mode option round-trips through onSaveField", () => {
    for (const mode of ["truecolor", "palette"] as const) {
      const { onSaveField, container } = mount({
        name: "default",
        color_mode: mode === "truecolor" ? "palette" : "truecolor",
      });
      const colorSelect = container.querySelectorAll("select")[1];
      fireEvent.change(colorSelect, { target: { value: mode } });
      expect(onSaveField).toHaveBeenCalledWith("theme", "color_mode", mode);
    }
  });
});
