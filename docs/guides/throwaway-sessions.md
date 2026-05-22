# Throwaway sessions

A throwaway session is a session that does not belong to any project on
disk. When you start one, AoE provisions a fresh temporary directory
under your system's temp folder, attaches the session to it, and
removes the directory when you delete the session.

Use throwaway sessions for one-off questions, quick investigations, or
ad-hoc agent runs where you do not want a session record tied to a
specific repo or to keep stray files around afterwards.

## When to use it

* You want to ask the agent a question that does not depend on a
  particular codebase.
* You want a clean, empty scratch directory for the agent to write
  files into without polluting an existing project.
* You want to investigate something quickly and have the directory
  go away automatically when you are done.

## Three ways to start one

### Command line

```bash
aoe add --throwaway -t "Quick question" -c claude
# or the short flag
aoe add -T -t "Quick question" -c claude
```

The session prints its resolved `Path:` line pointing at
`/tmp/aoe-throwaway-<id>` (Linux) or `$TMPDIR/aoe-throwaway-<id>`
(macOS). You do not pass a project path; it is provisioned for you.

Trying to pass a path alongside `--throwaway` is rejected:

```bash
aoe add /Users/me/repo --throwaway
# error: Cannot specify a project path with --throwaway
```

`--throwaway` is mutually exclusive with all worktree-related flags
(`-w`, `--new-branch`, `--base-branch`, `--repo`, `--project`,
`--no-submodules`). Mixing them fails at parse time with a clear
conflict message.

### Web dashboard

In the new-session wizard, the **Project** step has a toggle labeled
**Skip project folder** above the Recent / Browse / Clone URL tabs.
Turning it on hides the path picker and the worktree controls on the
Session step. The Review step shows
"Temporary directory (provisioned on create)" in place of the path.

Selecting a real project (Recent / Browse / Clone) turns the toggle
back off, so the wizard never submits a request with both a real path
and the throwaway flag set.

### TUI new-session dialog

Press `Ctrl+T` inside the new-session dialog from any field. The Path
input is replaced with a `(throwaway directory, Ctrl+T to undo)`
marker, the Worktree toggle is forced off, and submitting creates the
session in a fresh temp directory. Press `Ctrl+T` again to revert.

## What happens at delete

When you delete a throwaway session via `aoe rm`, the web dashboard
delete flow, or the TUI delete dialog, the deletion path also runs
`fs::remove_dir_all` on the session's temp directory. The cleanup is
guarded: it only runs when the session's `throwaway` flag is true AND
the path is under your OS temp dir AND its basename starts with
`aoe-throwaway-`. A session record with a tampered `project_path`
pointing at, say, `/etc` is left alone.

The wizard's **Recent projects** tab filters throwaway sessions out
once they exist, so a deleted throwaway directory does not appear as a
reusable recent project.

## Compatibility

* **Cockpit mode**: throwaway sessions work with `--cockpit` and the
  bundled ACP agents. The ACP worker spawns with the temp directory
  as its current working directory.
* **Sandboxes**: throwaway sessions can run in a container sandbox
  (`-s` or `--sandbox-image`); the container mounts the temp
  directory the same way it mounts a real project path.
* **Worktrees**: not supported. A throwaway directory is not a git
  repo, so the worktree concept does not apply. Use a regular project
  path with `-w` if you want a worktree.
* **Hooks**: a throwaway directory has no `.agent-of-empires/config.toml`,
  so the per-repo hook trust prompt never fires. Global and profile
  `on_create` hooks still run, with the temp directory as their `cwd`.

## Limits

If `aoe serve` (or your shell session) dies before you delete a
throwaway session, the directory is left on disk. Operating systems
clean their temp folders periodically, but a daemon-side orphan sweep
on `aoe serve` startup is tracked as a follow-up.

If you need the session to outlive its temp directory (rename, move,
keep the files), the recommended pattern is to copy whatever you need
out of the temp directory and recreate the session against a real path
with `aoe add <path>`.
