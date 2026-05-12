import { describe, expect, it } from "vitest";
import { lineDiffCounts } from "./diffStats";

describe("lineDiffCounts", () => {
  it("returns 0/0 for two empty strings", () => {
    expect(lineDiffCounts("", "")).toEqual({ adds: 0, dels: 0 });
  });

  it("counts a one-line single-character change as 1/1", () => {
    const old_ = ["line 1", "line 2", "line 3"].join("\n");
    const new_ = ["line 1", "line TWO", "line 3"].join("\n");
    expect(lineDiffCounts(old_, new_)).toEqual({ adds: 1, dels: 1 });
  });

  it("does not double-count shared context lines", () => {
    // Same 50-line body except line 25 changes character. Naive
    // total-line counting reports 50/50; real diff is 1/1.
    const a = Array.from({ length: 50 }, (_, i) => `line ${i}`).join("\n");
    const b = a.replace("line 25", "line TWENTY-FIVE");
    expect(lineDiffCounts(a, b)).toEqual({ adds: 1, dels: 1 });
  });

  it("counts a pure append as +N / -0", () => {
    const old_ = ["a", "b", "c"].join("\n");
    const new_ = ["a", "b", "c", "d", "e"].join("\n");
    expect(lineDiffCounts(old_, new_)).toEqual({ adds: 2, dels: 0 });
  });

  it("counts a pure deletion as +0 / -N", () => {
    const old_ = ["a", "b", "c", "d"].join("\n");
    const new_ = ["a", "d"].join("\n");
    expect(lineDiffCounts(old_, new_)).toEqual({ adds: 0, dels: 2 });
  });

  it("counts a top-of-block insertion as +1 / -0", () => {
    const old_ = ["a", "b", "c"].join("\n");
    const new_ = ["NEW", "a", "b", "c"].join("\n");
    expect(lineDiffCounts(old_, new_)).toEqual({ adds: 1, dels: 0 });
  });

  it("treats a single trailing newline as not adding a line", () => {
    expect(lineDiffCounts("a\nb", "a\nb\n")).toEqual({ adds: 0, dels: 0 });
    expect(lineDiffCounts("a\nb\n", "a\nb")).toEqual({ adds: 0, dels: 0 });
  });

  it("treats a pure-write (empty old) as adds-only", () => {
    const new_ = ["a", "b", "c"].join("\n");
    expect(lineDiffCounts("", new_)).toEqual({ adds: 3, dels: 0 });
  });

  it("treats a full-delete (empty new) as dels-only", () => {
    const old_ = ["a", "b", "c"].join("\n");
    expect(lineDiffCounts(old_, "")).toEqual({ adds: 0, dels: 3 });
  });
});
