export function applyBranchOverride(_title: string, worktreeBranch: string): {
  worktreeBranch: string;
  worktreeBranchDirty: boolean;
} {
  // Any direct edit on the branch field — including clearing it — marks it
  // dirty so the title→branch mirror stops overwriting the user's input on
  // the next keystroke. Empty is a valid UI state; the submit path falls
  // back to the title via getSubmittedBranch.
  return {
    worktreeBranch,
    worktreeBranchDirty: true,
  };
}

export function getSubmittedBranch(title: string, worktreeBranch: string): string {
  return worktreeBranch || title || "";
}

export function getReviewSummary(title: string, worktreeBranch: string): {
  title: string;
  branch: string;
} {
  return {
    title: title || worktreeBranch || "Auto-generated",
    branch: worktreeBranch || title || "Auto-generated",
  };
}
