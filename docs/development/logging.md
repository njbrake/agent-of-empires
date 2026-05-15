# Logging

Agent of Empires uses the [`tracing`](https://docs.rs/tracing) crate. The TUI, the `aoe serve` daemon, and the cockpit runner subprocesses all share `src/logging.rs` so they agree on env-var resolution, default filter construction, and the reloadable subscriber handle. TUI and runners append to the same `~/.agent-of-empires/debug.log` so a single tail covers an entire session; `aoe serve` writes to stdout (captured into `serve.log`).

## Targets

Targets use the convention `<module>.<submodule>`. The default filter expands a chosen level to a directive per top-level root, so target names like `auth.token` inherit from `auth` without per-target env config.

| Root | Sub-targets | What lands here |
|------|-------------|-----------------|
| `agent_of_empires` | (default crate path) | General library code emitting without `target:` |
| `cockpit` | `cockpit.acp`, `cockpit.supervisor`, `cockpit.event_store`, `cockpit.runner`, `cockpit.acp.stderr` | ACP transport, supervisor lifecycle, event store, runner shim |
| `terminal` | `terminal.ws`, `terminal.ws.bytes` | Web terminal WS relay + per-byte firehose (trace) |
| `auth` | `auth.token`, `auth.middleware`, `auth.rate_limit`, `auth.passphrase`, `auth.device`, `auth.ip` | Token rotate, middleware accept/reject, rate-limit thresholds, login flow |
| `process` | `process.signal`, `process.tree`, `process.reap`, `process.ppid` | Signal sends, process-tree walks, survivor reap, ppid resolution |
| `update` | `update.fetch`, `update.cache`, `update.parse` | GitHub release polling, cache hits/misses, version compare |
| `containers` | `containers.docker`, `containers.image`, `containers.runtime` | Docker daemon, image pull, container lifecycle |
| `git` | `git.command` | Every `git` invocation with args, exit, duration |
| `migrations` | (none — entry/exit on driver) | Per-migration progress with duration |
| `web.client` | (fixed; module surfaced as `client_target` field) | Browser-side errors relayed via `/api/client-log` |
| `log.runtime` | — | Filter swaps (REST + runner file-watch) |

## Levels

- **error** — user-visible failure or invariant violation
- **warn** — recovered from, but worth investigating (rate-limit lockout, SIGTERM survivor, garbled runtime-filter file)
- **info** — lifecycle / state transitions (token rotate, container start, migration completed)
- **debug** — frequent / per-operation detail (every git invocation, every signal)
- **trace** — per-byte / per-message firehose (`terminal.ws.bytes`, ACP JSON-RPC transport)

## Env variables

Resolved once at startup in `LogConfig::from_env`:

| Var | Effect |
|-----|--------|
| `AOE_LOG_LEVEL` | `trace`/`debug`/`info`/`warn`/`error`. Sets the level across all known target roots. |
| `AGENT_OF_EMPIRES_DEBUG` | Legacy alias for `AOE_LOG_LEVEL=debug`. |
| `AOE_ACP_TRACE` | Overlay: `agent_client_protocol=debug` + the JSON-RPC transport_actor at trace. |
| `AOE_TERMINAL_TRACE` | Overlay: `terminal=trace` (per-byte firehose). |

## Sinks by process

| Invocation | Filter source | Sink |
|---|---|---|
| Any `aoe` with env var set | env | `debug.log` |
| `aoe serve`, no env | `[logging]` config, else info baseline | stdout (captured into `serve.log` by the daemon redirect) |
| TUI (`aoe` with no subcommand), no env | `[logging]` config, else info baseline | `debug.log` |
| Cockpit runner subprocess | env (inherited) → `[logging]` config → info baseline; then `runtime_filter` if the daemon writes one | `debug.log` |
| Other one-shot CLI, no env | — | no subscriber installed (opt in via `AOE_LOG_LEVEL` if you need a trace of one) |

The TUI writes to a file rather than stderr because ratatui owns the alt-screen and a stderr subscriber would corrupt it. Daemon + runners + TUI all append to the same `debug.log` so a single tail covers a whole session.

## Persistent configuration (`[logging]` in `config.toml`)

The settings UI (web dashboard *Settings → Logging*, TUI *Settings → Logging*) writes a `[logging]` section to `~/.agent-of-empires/config.toml`:

```toml
[logging]
default_level = "info"

[logging.targets]
"cockpit.acp" = "trace"
"auth.middleware" = "debug"
"process.signal" = "warn"
```

`default_level` is the baseline; entries in `targets` override per target. The list of targets surfaced as dropdowns mirrors `KNOWN_SUB_TARGETS` in `src/logging.rs`. Anything else can still be set via raw EnvFilter syntax through the runtime endpoint or CLI.

Changes via the settings UI live-apply through the same `FilterController` swap that powers `aoe log-level`, including propagation to cockpit runners via the `runtime_filter` notify watcher. No daemon restart needed. (The startup precedence is shown in the *Sinks by process* table above: env wins, then `[logging]`, then the info baseline.)

## Runtime control

### REST

| Method | Path | Body | Notes |
|--------|------|------|-------|
| `GET` | `/api/log-level` | — | `{current, reloadable, ephemeral}`. Returns 200 even when no controller is installed. |
| `PATCH` | `/api/log-level` | `{"level": "<name>"}` or `{"filter": "<EnvFilter>"}` | Exactly one field. |

`{"level": "debug"}` expands across all known target roots so you don't accidentally enable debug for transitive crates like `hyper`/`rustls`/`tower`.

`{"filter": "..."}` accepts the full `EnvFilter` syntax. Regex matching is disabled (`with_regex(false)`) to reduce attack surface on an authenticated HTTP input. Bare global levels (`"filter": "debug"`) are rejected with 400; use the `level` form instead.

Responses include `previous` and `current` directives. Changes are ephemeral; restart falls back to the env-var resolution.

### CLI

```
aoe log-level <level>             # safe expansion across known roots
aoe log-level --filter <expr>     # raw EnvFilter
aoe log-level --get               # print current
```

Examples:

```sh
aoe log-level debug
aoe log-level --filter cockpit.acp=trace,info
aoe log-level --filter auth.rate_limit=debug,warn
aoe log-level --get
```

The CLI reads the daemon URL from `serve.url` and authenticates via the token in the query string. Works against a foreground `aoe serve` too — it does not require the daemon mode.

## Runner propagation

When you `PATCH /api/log-level`, the daemon writes the new directive atomically to `~/.agent-of-empires/runtime_filter` (0600). Each cockpit runner subprocess uses `notify` to watch the file and applies updates to its own `FilterController`. Both daemon and runners stay in lockstep without restart, which is the point: you can pull from `info` to `trace` mid-incident without losing in-flight agent state.

Edge cases:

- File missing: runner no-ops until the daemon writes one.
- Daemon stop: file removal does not revert runner filters; the next daemon start writes a fresh value.
- Garbled content: runner logs a `warn` to `log.runtime` and keeps its prior filter.

## Web client relay

Browser-side `window.onerror`, `unhandledrejection`, React `ErrorBoundary`, and explicit `reportError()` calls are batched and POSTed to `/api/client-log`. The server re-emits them through `tracing` at target `web.client` so they land in the same `debug.log` as everything else.

Throttle (frontend): token-bucket 10 cap, 10/s refill, batches flush every 2s / 20 entries / ~48 KB. `pagehide` and `visibilitychange === "hidden"` flush via `navigator.sendBeacon` with a JSON Blob so logs survive page navigation.

Caps (server): max 50 entries per batch (413 otherwise), message truncated to 4 KB, stack to 16 KB, dynamic-target field sanitised and capped at 64 characters. URL is sanitised client-side to drop the `?token=` query param before transmission.

Not captured (intentional, v1): `console.error`. Wrapping it produces noisy duplicates and recursion hazards; if you need it later, flag-gate it.

## File locations

| Path | When it's written |
|------|-------------------|
| `~/.agent-of-empires/debug.log` | TUI, cockpit runners, and any `aoe` invocation with `AOE_LOG_LEVEL` set. `aoe serve` writes its own output to stdout (captured into `serve.log`). |
| `~/.agent-of-empires/serve.log` | Daemon stdout/stderr tail captured by the `aoe serve --daemon` redirect. |
| `~/.agent-of-empires/runtime_filter` | Atomically written on every successful `aoe log-level` swap; consumed by runner watchers. |
| `~/.agent-of-empires/cockpit-workers/<session-id>.log` | Touched for compatibility; structured tracing now lands in the shared `debug.log`. |

On Linux, replace `~/.agent-of-empires` with `$XDG_CONFIG_HOME/agent-of-empires`. Debug builds use `~/.agent-of-empires-dev` to avoid colliding with an installed release.

## Conventions

- Set `target:` explicitly when filtering granularity below the crate level matters. The default crate path (`agent_of_empires`) suffices for grab-bag logs.
- Use structured fields, not interpolated text: `tracing::warn!(target: "auth.rate_limit", ip = %addr, attempts = n, "lockout")` rather than `warn!("lockout for ip {addr} ({n} attempts)")`. Field-based filtering and grep both win.
- Don't log secrets. Token material is never logged; auth events carry a `reason` field instead. Git command args are redacted (`https://***@host/...`) before tracing emit.
- Add new top-level target roots to `DEFAULT_TARGET_ROOTS` in `src/logging.rs` so the runtime control's `{"level": ...}` expansion picks them up.
