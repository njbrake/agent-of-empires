// @vitest-environment jsdom
//
// Unit tests for readThemeFromCss. The function maps --term-* CSS
// custom properties on documentElement (set by useResolvedTheme during
// runtime palette swap) to the ITheme object xterm.js consumes. Bug
// here = theme change events don't take effect on the live terminal,
// or the terminal paints with stale ANSI slots after a reload.

import { describe, expect, it, beforeEach } from "vitest";
import { readThemeFromCss } from "./useTerminal";

function setTermVars(vars: Record<string, string>) {
  const root = document.documentElement;
  for (const [k, v] of Object.entries(vars)) {
    root.style.setProperty(k, v);
  }
}

describe("readThemeFromCss", () => {
  beforeEach(() => {
    // Clear any --term-* leftovers from a previous test so missing slots
    // exercise the fallback path.
    const root = document.documentElement;
    for (const prop of Array.from(root.style)) {
      if (prop.startsWith("--term-")) root.style.removeProperty(prop);
    }
  });

  it("falls back to the bundled defaults when nothing is set", () => {
    const theme = readThemeFromCss();
    expect(theme.background).toBe("#1c1c1f");
    expect(theme.foreground).toBe("#e4e4e7");
    expect(theme.cursor).toBe("#f59e0b");
    // cursorAccent matches background so the cursor reads against any bg
    expect(theme.cursorAccent).toBe("#1c1c1f");
    expect(theme.black).toBe("#1c1c1f");
    expect(theme.red).toBe("#ef4444");
    expect(theme.brightWhite).toBe("#fbbf24");
  });

  it("projects --term-bg / --term-fg / --term-cursor through", () => {
    setTermVars({
      "--term-bg": "#101010",
      "--term-fg": "#fafafa",
      "--term-cursor": "#ff00ff",
    });
    const theme = readThemeFromCss();
    expect(theme.background).toBe("#101010");
    expect(theme.foreground).toBe("#fafafa");
    expect(theme.cursor).toBe("#ff00ff");
    // cursorAccent reads from the same --term-bg slot
    expect(theme.cursorAccent).toBe("#101010");
  });

  it("projects every --term-color-N slot to the matching ANSI key", () => {
    const palette: Record<string, string> = {};
    for (let i = 0; i < 16; i++) {
      // Pick distinct sentinel colors so a swap with the wrong key is
      // caught (e.g. red <-> brightRed would otherwise look fine).
      palette[`--term-color-${i}`] = `#${i.toString(16).padStart(2, "0")}aabb`;
    }
    setTermVars(palette);
    const theme = readThemeFromCss();
    expect(theme.black).toBe("#00aabb");
    expect(theme.red).toBe("#01aabb");
    expect(theme.green).toBe("#02aabb");
    expect(theme.yellow).toBe("#03aabb");
    expect(theme.blue).toBe("#04aabb");
    expect(theme.magenta).toBe("#05aabb");
    expect(theme.cyan).toBe("#06aabb");
    expect(theme.white).toBe("#07aabb");
    expect(theme.brightBlack).toBe("#08aabb");
    expect(theme.brightRed).toBe("#09aabb");
    expect(theme.brightGreen).toBe("#0aaabb");
    expect(theme.brightYellow).toBe("#0baabb");
    expect(theme.brightBlue).toBe("#0caabb");
    expect(theme.brightMagenta).toBe("#0daabb");
    expect(theme.brightCyan).toBe("#0eaabb");
    expect(theme.brightWhite).toBe("#0faabb");
  });

  it("trims whitespace in CSS variable values", () => {
    // getComputedStyle returns "  #abc  " with leading/trailing
    // whitespace for some browser implementations; the projection must
    // strip it so xterm.js can parse the color.
    document.documentElement.style.setProperty("--term-bg", "  #abcdef  ");
    expect(readThemeFromCss().background).toBe("#abcdef");
  });

  it("uses the fallback when an explicit empty value is set", () => {
    // An empty string from getComputedStyle should resolve to the
    // hard-coded default, otherwise the terminal would paint with an
    // empty ITheme slot (xterm.js treats that as transparent).
    document.documentElement.style.setProperty("--term-bg", "");
    expect(readThemeFromCss().background).toBe("#1c1c1f");
  });
});
