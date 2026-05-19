# Storage and concurrency

`src/session/storage.rs` is the only persistence layer for `sessions.json`,
`groups.json`, and `workspace-ordering.json`. Every writer in the process,
TUI main thread, server `spawn_blocking` workers, the cockpit ACP
listener, the session-id poller drain, the CLI, the `restart --all`
JoinSet, goes through it.

## Atomicity guarantees

### File atomicity (since #1208)

Every write goes through `atomic_write` (private to the module): write
content to a sibling `tempfile::NamedTempFile`, `sync_data`, atomic rename
into place, best-effort `sync_all` on the parent directory. A reader can
never observe a torn or partial file; it sees either the prior or the
next snapshot.

### Read-modify-write atomicity (since #1175)

`Storage::update` and `Storage::commit` hold a per-profile `std::sync::Mutex`
across the full `load -> mutate -> save` cycle. The mutex is registered
process-wide in a `OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>>`, so
every caller that constructs `Storage::new(profile)` for the same profile
shares the same `Arc`. Workspace ordering has its own dedicated global
mutex, since the file lives at the app-data root rather than per profile.

The closure passed to `update` is `FnOnce(&mut Vec<Instance>, &mut Vec<Group>)
-> Result<R>` and is fully synchronous. A `std::sync::Mutex` is safe across
the body even on the tokio multi-threaded runtime: server callers wrap
`update` in `tokio::task::spawn_blocking`, which is the existing pattern.

`save_workspace_ordering` is `pub(crate)` and exists only to be called by
`update_workspace_ordering` internally. The per-profile `save` and
`save_groups` helpers have been removed entirely. External crates and
integration tests cannot bypass the lock from outside the crate.

## Choosing between `update` and `commit`

| Caller owns authoritative in-memory state? | Use |
|---|---|
| No (server REST handler, `instance.rs`, CLI, cockpit listener) | `Storage::update(closure)` |
| Yes (TUI `HomeView`'s `Vec<Instance>` is the source of truth) | `Storage::commit(&instances, &group_tree)` |

`update` is the safer default: it loads the current disk state, hands it
to the closure for mutation, and writes it back, so concurrent mutations
to other fields by other writers are preserved. `commit` overwrites
wholesale with the caller's in-memory state, last-writer-wins; that is
the correct semantics for HomeView (only writer in the TUI process for
its profile slice) and wrong for handlers that filter `state.instances`
into a slice and save it.

## Lock ordering

When a server handler holds `state.instances` (tokio `RwLock`) AND needs
to call `Storage::update` / `commit`, the order is always:

1. Acquire `state.instances` write guard.
2. Build the data needed for the disk write.
3. Drop the `state.instances` guard.
4. Inside `tokio::task::spawn_blocking`, call `Storage::update` /
   `commit` (acquires the per-profile sync mutex).
5. After `spawn_blocking` returns, optionally re-acquire
   `state.instances` to refresh in-memory state.

Never `Storage::update` while holding `state.instances`. Never the
reverse either: holding the storage mutex while awaiting on the tokio
RwLock would block a blocking thread for an indefinite async wait.

## Cross-process limitation

The per-profile mutex is process-local. If `aoe` (TUI) and `aoe serve`
write to the same profile simultaneously, lost updates are still
possible: both processes read disjoint snapshots, both save, the second
clobbers the first. The recommended workflow is to use either the TUI or
`aoe serve` against a given profile, not both. A future advisory
`flock(LOCK_EX)` on `sessions.json` around the same locked save path
would close this; tracked as a follow-up to #1175.

## Crash atomicity across `sessions.json` and `groups.json`

`update` / `commit` write the two files in sequence under the same lock:
no in-process reader sees an inconsistent pair, but a process death
between the two `atomic_write` calls can still leave `groups.json`
referencing instances missing from `sessions.json` (or vice versa).
`GroupTree::new_with_groups` tolerates orphan group paths and orphan
instances on the next load, so this manifests as a reconciliation, not
data loss. Closing the window properly would require a single combined
manifest file, a schema change beyond the scope of #1175.

## Adding new persisted state

If you add a new file (a `notes.json`, a `prompts.json`, ...): give it
its own `update_*` helper following the pattern of
`update_workspace_ordering`. Each persisted file gets a dedicated lock
so unrelated files do not serialise against each other.

If you add a new `#[serde(skip)]` field to `Instance`: extend
`merge_runtime_fields` in `src/server/mod.rs` to carry it across the
2-second poll-loop reload. Otherwise the field is silently reset to
default every poll tick.
