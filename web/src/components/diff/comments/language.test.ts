import { describe, it, expect } from "vitest";
import { extensionToLanguage } from "./language";

describe("extensionToLanguage", () => {
  it.each([
    ["src/main.rs", "rust"],
    ["src/App.tsx", "tsx"],
    ["src/lib/api.ts", "ts"],
    ["foo.py", "python"],
    ["build.sh", "bash"],
    ["docker/Dockerfile", "dockerfile"],
    ["DOCKERFILE", "dockerfile"],
    ["nested/path/file.json", "json"],
    ["foo.YAML", "yaml"],
    ["readme.md", "markdown"],
  ])("maps %s -> %s", (path, lang) => {
    expect(extensionToLanguage(path)).toBe(lang);
  });

  it("returns empty for unknown extensions", () => {
    expect(extensionToLanguage("foo.xyz")).toBe("");
  });

  it("returns empty for extensionless files", () => {
    expect(extensionToLanguage("Makefile")).toBe("");
    expect(extensionToLanguage("README")).toBe("");
  });

  it("handles paths without slashes", () => {
    expect(extensionToLanguage("foo.rs")).toBe("rust");
  });
});
