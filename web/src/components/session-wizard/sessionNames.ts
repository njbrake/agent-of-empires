export function applyBranchOverride(title: string, worktreeBranch: string): {
  worktreeBranch: string;
  worktreeBranchDirty: boolean;
} {
  if (worktreeBranch === "") {
    return {
      worktreeBranch: title,
      worktreeBranchDirty: false,
    };
  }

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
