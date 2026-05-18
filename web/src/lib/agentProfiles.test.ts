import { describe, expect, it } from "vitest";
import {
  DEFAULT_AGENT_PROFILE,
  resolveAgentProfile,
} from "./agentProfiles";

describe("resolveAgentProfile", () => {
  it("resolves known agent keys", () => {
    expect(resolveAgentProfile("claude").key).toBe("claude");
    expect(resolveAgentProfile("claude-code").key).toBe("claude-code");
    expect(resolveAgentProfile("codex").key).toBe("codex");
    expect(resolveAgentProfile("opencode").key).toBe("opencode");
    expect(resolveAgentProfile("gemini").key).toBe("gemini");
    expect(resolveAgentProfile("vibe").key).toBe("vibe");
    expect(resolveAgentProfile("pi").key).toBe("pi");
    expect(resolveAgentProfile("aoe-agent").key).toBe("aoe-agent");
  });

  it("falls back to DEFAULT for unknown / nullish keys", () => {
    expect(resolveAgentProfile(undefined).key).toBe(DEFAULT_AGENT_PROFILE.key);
    expect(resolveAgentProfile(null).key).toBe(DEFAULT_AGENT_PROFILE.key);
    expect(resolveAgentProfile("").key).toBe(DEFAULT_AGENT_PROFILE.key);
    expect(resolveAgentProfile("custom").key).toBe(DEFAULT_AGENT_PROFILE.key);
  });

  it("claude has all specialised UI capabilities enabled", () => {
    const p = resolveAgentProfile("claude");
    expect(p.capabilities.todos).toBe(true);
    expect(p.capabilities.skills).toBe(true);
    expect(p.capabilities.wakeup).toBe(true);
    expect(p.parentMetaNamespaces).toEqual(["claudeCode"]);
  });

  it("codex / opencode / gemini disable claude-specific cards", () => {
    for (const key of ["codex", "opencode", "gemini"] as const) {
      const p = resolveAgentProfile(key);
      expect(p.capabilities.todos).toBe(false);
      expect(p.capabilities.skills).toBe(false);
      expect(p.capabilities.wakeup).toBe(false);
      expect(p.parentMetaNamespaces).toEqual([]);
    }
  });

  it("codex aliases route shell / apply_patch / view_file to canonical cards", () => {
    const p = resolveAgentProfile("codex");
    expect(p.aliases.execute).toEqual(["shell", "bash"]);
    expect(p.aliases.edit).toEqual(["apply_patch"]);
    expect(p.aliases.read).toContain("view_file");
  });

  it("opencode aliases cover bash / read / edit / write / grep / glob / webfetch / task", () => {
    const p = resolveAgentProfile("opencode");
    expect(p.aliases.execute).toEqual(["bash"]);
    expect(p.aliases.edit).toEqual(["edit", "write"]);
    expect(p.aliases.search).toEqual(["grep", "glob"]);
    expect(p.aliases.fetch).toEqual(["webfetch"]);
    expect(p.aliases.think).toEqual(["task"]);
  });

  it("gemini aliases cover run_shell_command / read_file / web_fetch", () => {
    const p = resolveAgentProfile("gemini");
    expect(p.aliases.execute).toEqual(["run_shell_command"]);
    expect(p.aliases.read).toContain("read_file");
    expect(p.aliases.read).toContain("read_many_files");
    expect(p.aliases.fetch).toEqual(["web_fetch"]);
  });

  it("clearAliases match the server-side rust profile", () => {
    expect(resolveAgentProfile("claude").clearAliases).toEqual(["/clear"]);
    expect(resolveAgentProfile("codex").clearAliases).toEqual(["/new"]);
    expect(resolveAgentProfile("opencode").clearAliases).toEqual(["/new"]);
    expect(resolveAgentProfile("gemini").clearAliases).toEqual([]);
  });
});
