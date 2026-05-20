// Git fixture helpers for live Playwright specs.
//
// Used by `tests/live/git-clone.spec.ts` (and any future spec exercising
// the `POST /api/git/clone` endpoint) to spin up a throwaway bare repo
// the wizard can clone from via `file://`. Keeping the source repo on
// the local filesystem keeps the test deterministic, hermetic, and free
// from any network dependency.
//
// The matching server-side validator accepts `file://` URLs by design.
// See `src/server/api/git.rs::looks_like_git_url` for the allowlist.

import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { join } from "node:path";

export interface BareRepoFixture {
  /** Absolute path of the bare repo on disk. */
  path: string;
  /** `file://` URL pointing at `path`, ready to feed into the wizard input. */
  url: string;
}

/**
 * Create a throwaway local bare git repo so a live `aoe serve` can clone
 * from `file://...`. Parent dir must already exist (the harness home tree
 * is created before this helper runs).
 *
 * Returns the absolute path and the `file://` URL.
 */
export function createBareRepo(parentDir: string, name = "bare.git"): BareRepoFixture {
  const path = join(parentDir, name);
  mkdirSync(parentDir, { recursive: true });
  const res = spawnSync("git", ["init", "--bare", "--quiet", path], {
    env: {
      ...process.env,
      GIT_AUTHOR_NAME: "t",
      GIT_AUTHOR_EMAIL: "t@t",
      GIT_COMMITTER_NAME: "t",
      GIT_COMMITTER_EMAIL: "t@t",
    },
  });
  if (res.status !== 0) {
    throw new Error(
      `git init --bare failed: status=${res.status} stderr=${res.stderr?.toString() ?? "<none>"}`,
    );
  }
  return { path, url: `file://${path}` };
}
