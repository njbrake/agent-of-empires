import { describe, it, expect } from "vitest";
import { buildCommentsMarkdown, buildFullPrompt } from "./buildPrompt";
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
    expect(prompt.startsWith("Hey:\n\n## Diff comments")).toBe(true);
  });

  it("trims surrounding whitespace from intro/outro", () => {
    const prompt = buildFullPrompt(
      [mk({})],
      "  intro  \n",
      "   outro   ",
      { isMultiRepo: false },
    );
    expect(prompt.startsWith("intro\n")).toBe(true);
    expect(prompt).toMatch(/outro\n$/);
  });

  it("omits the comments section when no comments", () => {
    const prompt = buildFullPrompt([], "intro", "outro", { isMultiRepo: false });
    expect(prompt).not.toContain("## Diff comments");
    expect(prompt).toContain("intro");
    expect(prompt).toContain("outro");
  });
});
