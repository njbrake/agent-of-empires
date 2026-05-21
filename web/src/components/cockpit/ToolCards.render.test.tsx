// @vitest-environment jsdom
//
// Per-kind dispatch coverage for ToolCards. Today only the
// `formatDurationMs` helper has a unit test; every per-kind render
// branch (bash, read, edit, search, todo, skill, schedule, mcp,
// generic) is uncovered. This spec pins each branch to a label / DOM
// shape so a future refactor that drops or misroutes a card surfaces
// here loudly.
//
// We render via the public `<ToolCard>` dispatcher because the
// per-kind functions (ExecuteToolCard, EditToolCard, etc.) are not
// exported. Shiki is mocked away so HighlightedBlock falls through to
// a plain <pre> and the test doesn't depend on async theme loading.

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import type { ReactNode } from "react";

vi.mock("../../lib/highlighter", () => ({
  ensureThemeLoaded: vi.fn().mockResolvedValue("dark-plus"),
  getHighlighter: vi.fn().mockResolvedValue({
    codeToHtml: (code: string) => `<pre>${code}</pre>`,
  }),
  langKeyForExt: (s: string) => s,
  loadLanguage: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../hooks/useShikiTheme", () => ({
  useShikiTheme: () => ({ theme: "dark-plus", appearance: "dark" }),
}));

import { ToolCard } from "./ToolCards";
import { AgentProfileProvider } from "../../lib/agentProfileContext";
import { fixtures, makeCompletion, makeError } from "./__fixtures__/toolCalls";

function Wrap({
  toolKey,
  children,
}: {
  toolKey?: string;
  children: ReactNode;
}) {
  return (
    <AgentProfileProvider toolKey={toolKey ?? null}>
      {children}
    </AgentProfileProvider>
  );
}

afterEach(() => {
  cleanup();
});

describe("ToolCards dispatch", () => {
  it("renders bash kind with a 'bash' label and the command", () => {
    const { container } = render(
      <Wrap>
        <ToolCard tool={fixtures.bash} result={undefined} />
      </Wrap>,
    );
    expect(container.textContent).toContain("bash");
    expect(container.textContent).toContain("ls -la");
  });

  it("renders read kind with a 'read' label and the file path", () => {
    const { container } = render(
      <Wrap>
        <ToolCard tool={fixtures.read} result={undefined} />
      </Wrap>,
    );
    expect(container.textContent).toContain("read");
    expect(container.textContent).toContain("/tmp/main.rs");
  });

  it("renders edit kind with an 'edit' label when old_string is non-empty", () => {
    const { container } = render(
      <Wrap>
        <ToolCard tool={fixtures.edit} result={undefined} />
      </Wrap>,
    );
    expect(container.textContent).toContain("edit");
    expect(container.textContent).toContain("/tmp/main.rs");
  });

  it("renders edit kind with a 'write' label when old_string is empty", () => {
    const { container } = render(
      <Wrap>
        <ToolCard tool={fixtures.write} result={undefined} />
      </Wrap>,
    );
    expect(container.textContent).toContain("write");
    expect(container.textContent).toContain("/tmp/new.rs");
  });

  it("renders delete kind with a 'delete' label", () => {
    const { container } = render(
      <Wrap>
        <ToolCard tool={fixtures.del} result={undefined} />
      </Wrap>,
    );
    expect(container.textContent).toContain("delete");
    expect(container.textContent).toContain("/tmp/gone.rs");
  });

  it("renders search kind with a 'search' label", () => {
    const { container } = render(
      <Wrap>
        <ToolCard tool={fixtures.search} result={undefined} />
      </Wrap>,
    );
    expect(container.textContent).toContain("search");
  });

  it("renders fetch kind with a 'fetch' label and the URL", () => {
    const { container } = render(
      <Wrap>
        <ToolCard tool={fixtures.fetch} result={undefined} />
      </Wrap>,
    );
    expect(container.textContent).toContain("fetch");
    expect(container.textContent).toContain("example.com");
  });

  it("renders generic kind for unrecognised tool names", () => {
    const { container } = render(
      <Wrap>
        <ToolCard tool={fixtures.generic} result={undefined} />
      </Wrap>,
    );
    // Generic falls back to tool.kind as the label.
    expect(container.textContent).toContain("WeirdTool");
  });

  it("flips the status pill to 'failed' on tool_error results", () => {
    const { container } = render(
      <Wrap>
        <ToolCard
          tool={fixtures.bash}
          result={makeError({ text: "command not found" })}
        />
      </Wrap>,
    );
    expect(container.textContent?.toLowerCase()).toContain("failed");
  });

  it("renders the 'done' badge on tool_complete results", () => {
    const { container } = render(
      <Wrap>
        <ToolCard
          tool={fixtures.bash}
          result={makeCompletion({ text: "hello\n" })}
        />
      </Wrap>,
    );
    expect(container.textContent).toContain("done");
  });
});

describe("ToolCards profile-gated dispatch (claude)", () => {
  it("routes TodoWrite to the todos card under the claude profile", () => {
    const { container } = render(
      <Wrap toolKey="claude">
        <ToolCard tool={fixtures.todoWrite} result={undefined} />
      </Wrap>,
    );
    expect(container.textContent).toContain("todos");
    expect(container.textContent).toContain("Step one");
    expect(container.textContent).toContain("Step two");
    expect(container.textContent).toContain("Step three");
  });

  it("routes a Skill tool to the skill card under the claude profile", () => {
    const { container } = render(
      <Wrap toolKey="claude">
        <ToolCard tool={fixtures.skill} result={undefined} />
      </Wrap>,
    );
    expect(container.textContent).toContain("skill");
  });

  it("routes ScheduleWakeup to a wakeup card under the claude profile", () => {
    const { container } = render(
      <Wrap toolKey="claude">
        <ToolCard tool={fixtures.scheduleWakeup} result={undefined} />
      </Wrap>,
    );
    expect(container.textContent).toContain("checking deploy");
  });
});

describe("ToolCards MCP", () => {
  it("renders an MCP card with the server name and verb", () => {
    const { container } = render(
      <Wrap toolKey="claude">
        <ToolCard tool={fixtures.mcp} result={undefined} />
      </Wrap>,
    );
    expect(container.textContent?.toLowerCase()).toContain("mcp");
    expect(container.textContent?.toLowerCase()).toContain("slack");
    expect(container.textContent?.toLowerCase()).toContain("send message");
  });
});
