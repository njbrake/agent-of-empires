import { describe, expect, it } from "vitest";
import {
  applyBranchOverride,
  getReviewSummary,
  getSubmittedBranch,
} from "./sessionNames";

describe("applyBranchOverride", () => {
  it("marks a non-empty branch as a manual override", () => {
    expect(applyBranchOverride("session-title", "feature/custom")).toEqual({
      worktreeBranch: "feature/custom",
      worktreeBranchDirty: true,
    });
  });

  it("resets an empty branch back to the title-derived default", () => {
    expect(applyBranchOverride("session-title", "")).toEqual({
      worktreeBranch: "session-title",
      worktreeBranchDirty: false,
    });
  });

  it("keeps both fields empty when there is no title to fall back to", () => {
    expect(applyBranchOverride("", "")).toEqual({
      worktreeBranch: "",
      worktreeBranchDirty: false,
    });
  });
});

describe("getSubmittedBranch", () => {
  it("prefers the explicit branch override", () => {
    expect(getSubmittedBranch("session-title", "feature/custom")).toBe(
      "feature/custom",
    );
  });

  it("falls back to the title when the branch field is cleared", () => {
    expect(getSubmittedBranch("session-title", "")).toBe("session-title");
  });

  it("leaves the branch empty only when both fields are empty", () => {
    expect(getSubmittedBranch("", "")).toBe("");
  });
});

describe("getReviewSummary", () => {
  it("shows the branch when the title is blank because the backend reuses it", () => {
    expect(getReviewSummary("", "feature/custom")).toEqual({
      title: "feature/custom",
      branch: "feature/custom",
    });
  });

  it("shows the title-derived branch when no explicit branch is set", () => {
    expect(getReviewSummary("session-title", "")).toEqual({
      title: "session-title",
      branch: "session-title",
    });
  });
});
