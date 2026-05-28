import { describe, expect, it } from "vitest";
import { clampMenuPosition } from "../menuPosition";

describe("clampMenuPosition", () => {
  const VW = 1000;
  const VH = 800;

  it("returns the anchor unchanged when the menu fits at the cursor", () => {
    const out = clampMenuPosition({
      x: 100,
      y: 100,
      menuWidth: 200,
      menuHeight: 300,
      viewportWidth: VW,
      viewportHeight: VH,
    });
    expect(out).toEqual({ x: 100, y: 100 });
  });

  it("clamps top when the menu would overflow the bottom edge", () => {
    const out = clampMenuPosition({
      x: 100,
      y: 750,
      menuWidth: 200,
      menuHeight: 300,
      viewportWidth: VW,
      viewportHeight: VH,
    });
    expect(out.y).toBe(VH - 300 - 8);
    expect(out.x).toBe(100);
  });

  it("clamps left when the menu would overflow the right edge", () => {
    const out = clampMenuPosition({
      x: 950,
      y: 100,
      menuWidth: 200,
      menuHeight: 300,
      viewportWidth: VW,
      viewportHeight: VH,
    });
    expect(out.x).toBe(VW - 200 - 8);
    expect(out.y).toBe(100);
  });

  it("clamps both axes when the menu would overflow the bottom-right corner", () => {
    const out = clampMenuPosition({
      x: 950,
      y: 750,
      menuWidth: 200,
      menuHeight: 300,
      viewportWidth: VW,
      viewportHeight: VH,
    });
    expect(out).toEqual({ x: VW - 200 - 8, y: VH - 300 - 8 });
  });

  it("collapses to the margin when the menu is taller than the viewport", () => {
    const out = clampMenuPosition({
      x: 50,
      y: 50,
      menuWidth: 200,
      menuHeight: 5000,
      viewportWidth: VW,
      viewportHeight: VH,
    });
    expect(out.y).toBe(8);
  });

  it("honors a custom margin", () => {
    const out = clampMenuPosition({
      x: 950,
      y: 100,
      menuWidth: 200,
      menuHeight: 100,
      viewportWidth: VW,
      viewportHeight: VH,
      margin: 16,
    });
    expect(out.x).toBe(VW - 200 - 16);
  });

  it("clamps negative anchors up to the margin", () => {
    const out = clampMenuPosition({
      x: -50,
      y: -30,
      menuWidth: 200,
      menuHeight: 300,
      viewportWidth: VW,
      viewportHeight: VH,
    });
    expect(out).toEqual({ x: 8, y: 8 });
  });
});
