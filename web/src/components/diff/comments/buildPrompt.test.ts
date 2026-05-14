import { describe, it, expect } from "vitest";
import {
  buildCommentsMarkdown,
  buildFullPrompt,
  parseDiffCommentsSentinel,
  stripDiffCommentsSentinel,
} from "./buildPrompt";
import type { DiffComment } from "./types";

function mk(partial: Partial<DiffComment>): DiffComment {
  return {
    id: "c1",
    filePath: "src/foo.rs",
    side: "new",
    startLine: 10,
    endLine: 10,
    body: "body",
    capturedSnippet: "let x = 1;",
    language: "rust",
    createdAt: "2025-01-01T00:00:00Z",
    ...partial,
  };
}

describe("buildCommentsMarkdown", () => {
  it("renders single-line wording", () => {
    const md = buildCommentsMarkdown([mk({})], { isMultiRepo: false });
    expect(md).toContain("### `src/foo.rs` line 10 (new)");
  });

  it("renders range wording", () => {
    const md = buildCommentsMarkdown(
      [mk({ startLine: 10, endLine: 14 })],
      { isMultiRepo: false },
    );
    expect(md).toContain("### `src/foo.rs` lines 10-14 (new)");
  });

  it("includes a code fence with language", () => {
    const md = buildCommentsMarkdown([mk({})], { isMultiRepo: false });
    expect(md).toContain("```rust\nlet x = 1;\n```");
  });

  it("expands the fence when snippet contains backticks", () => {
    const md = buildCommentsMarkdown(
      [
        mk({
          capturedSnippet: "before\n```\ninner\n```\nafter",
          language: "",
        }),
      ],
      { isMultiRepo: false },
    );
    expect(md).toMatch(/^### .*\n\n````\n/m);
    expect(md).toContain("```\ninner\n```");
  });

  it("prefixes repo when isMultiRepo", () => {
    const md = buildCommentsMarkdown(
      [mk({ repoName: "repoA" })],
      { isMultiRepo: true },
    );
    expect(md).toContain("### [repoA] `src/foo.rs`");
  });

  it("does not prefix repo when not multi-repo even if repoName set", () => {
    const md = buildCommentsMarkdown(
      [mk({ repoName: "repoA" })],
      { isMultiRepo: false },
    );
    expect(md).not.toContain("[repoA]");
  });

  it("sorts by repo, file, line, side, createdAt", () => {
    const md = buildCommentsMarkdown(
      [
        mk({ id: "c4", filePath: "src/b.rs", startLine: 5, endLine: 5 }),
        mk({ id: "c1", filePath: "src/a.rs", startLine: 20, endLine: 20 }),
        mk({ id: "c2", filePath: "src/a.rs", startLine: 5, endLine: 5 }),
        mk({
          id: "c3",
          filePath: "src/a.rs",
          startLine: 5,
          endLine: 5,
          side: "old",
        }),
      ],
      { isMultiRepo: false },
    );
    const order = md
      .split("\n")
      .filter((l) => l.startsWith("### "))
      .map((l) => l.slice("### ".length));
    expect(order).toEqual([
      "`src/a.rs` line 5 (old)",
      "`src/a.rs` line 5 (new)",
      "`src/a.rs` line 20 (new)",
      "`src/b.rs` line 5 (new)",
    ]);
  });

  it("returns empty string for no comments", () => {
    expect(buildCommentsMarkdown([], { isMultiRepo: false })).toBe("");
  });
});

describe("buildFullPrompt", () => {
  it("uses default outro when blank", () => {
    const prompt = buildFullPrompt([mk({})], "", "", { isMultiRepo: false });
    expect(prompt).toMatch(/Please address these comments\.\n$/);
  });

  it("uses provided outro when set", () => {
    const prompt = buildFullPrompt([mk({})], "", "Custom outro", {
      isMultiRepo: false,
    });
    expect(prompt).toMatch(/Custom outro\n$/);
    expect(prompt).not.toContain("Please address these comments.");
  });

  it("prepends intro when set", () => {
    const prompt = buildFullPrompt([mk({})], "Hey:", "", { isMultiRepo: false });
    const body = stripDiffCommentsSentinel(prompt);
    expect(body.startsWith("Hey:\n\n## Diff comments")).toBe(true);
  });

  it("trims surrounding whitespace from intro/outro", () => {
    const prompt = buildFullPrompt(
      [mk({})],
      "  intro  \n",
      "   outro   ",
      { isMultiRepo: false },
    );
    const body = stripDiffCommentsSentinel(prompt);
    expect(body.startsWith("intro\n")).toBe(true);
    expect(body).toMatch(/outro\n$/);
  });

  it("omits the comments section when no comments", () => {
    const prompt = buildFullPrompt([], "intro", "outro", { isMultiRepo: false });
    expect(prompt).not.toContain("## Diff comments");
    expect(prompt).not.toContain("aoe:diff-comments");
    expect(prompt).toContain("intro");
    expect(prompt).toContain("outro");
  });

  it("prepends a sentinel header when comments are present", () => {
    const prompt = buildFullPrompt([mk({})], "", "", { isMultiRepo: false });
    expect(prompt.startsWith("<!-- aoe:diff-comments:v1 ")).toBe(true);
    expect(prompt).toContain("\n## Diff comments\n");
  });
});

describe("parseDiffCommentsSentinel", () => {
  it("returns null when no sentinel", () => {
    expect(parseDiffCommentsSentinel("hello world")).toBeNull();
  });

  it("round-trips a payload built by buildFullPrompt", () => {
    const comment = mk({
      body: "needs error handling",
      capturedSnippet: "let x = 1;",
    });
    const prompt = buildFullPrompt([comment], "Take a look:", "Thanks.", {
      isMultiRepo: false,
    });
    const payload = parseDiffCommentsSentinel(prompt);
    expect(payload).not.toBeNull();
    expect(payload!.intro).toBe("Take a look:");
    expect(payload!.outro).toBe("Thanks.");
    expect(payload!.isMultiRepo).toBe(false);
    expect(payload!.comments).toHaveLength(1);
    expect(payload!.comments[0]!.body).toBe("needs error handling");
    expect(payload!.comments[0]!.capturedSnippet).toBe("let x = 1;");
  });

  it("survives snippets that contain `-->`", () => {
    const c = mk({ capturedSnippet: "<!-- not allowed -->\nlet x = 1;" });
    const prompt = buildFullPrompt([c], "", "", { isMultiRepo: false });
    const payload = parseDiffCommentsSentinel(prompt);
    expect(payload).not.toBeNull();
    expect(payload!.comments[0]!.capturedSnippet).toContain("-->");
  });

  it("returns null for a malformed payload", () => {
    const broken = "<!-- aoe:diff-comments:v1 not-base64!@# -->\nbody\n";
    expect(parseDiffCommentsSentinel(broken)).toBeNull();
  });

  it("preserves the visible body for the agent", () => {
    const prompt = buildFullPrompt([mk({})], "Intro:", "Outro.", {
      isMultiRepo: false,
    });
    const body = stripDiffCommentsSentinel(prompt);
    expect(body.startsWith("Intro:\n\n## Diff comments")).toBe(true);
    expect(body).toMatch(/Outro\.\n$/);
  });
});
