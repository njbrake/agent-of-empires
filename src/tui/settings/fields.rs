//! Setting field definitions and config mapping

use crate::session::{
    validate_check_interval, Config, ContainerRuntimeName, DefaultTerminalMode, ProfileConfig,
    TmuxClipboardMode, TmuxMouseMode, TmuxStatusBarMode,
};
use crate::sound::{
    validate_sound_exists, volume_from_option, volume_options, volume_to_index, SoundMode,
};
use crate::tui::styles::available_themes;

use super::SettingsScope;

/// Categories of settings
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCategory {
    Theme,
    Updates,
    Worktree,
    Sandbox,
    Tmux,
    Session,
    Sound,
    StatusHooks,
    Hooks,
    Web,
    Cockpit,
    Logging,
}

impl SettingsCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Theme => "Theme",
            Self::Updates => "Updates",
            Self::Worktree => "Worktree",
            Self::Sandbox => "Sandbox",
            Self::Tmux => "Tmux",
            Self::Session => "Session",
            Self::Sound => "Sound",
            Self::StatusHooks => "Status Hooks",
            Self::Hooks => "Hooks",
            Self::Web => "Web",
            Self::Cockpit => "Cockpit",
            Self::Logging => "Logging",
        }
    }
}

/// Type-safe field identifiers (prevents typos in string matching)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKey {
    // Profile (only relevant when the Profile scope is active)
    ProfileDescription,
    // Theme
    ThemeName,
    ThemeColorMode,
    IdleDecayMinutes,
    // Updates
    CheckEnabled,
    CheckIntervalHours,
    NotifyInCli,
    WebPollIntervalMinutes,
    // Worktree
    WorktreeEnabled,
    PathTemplate,
    BareRepoPathTemplate,
    WorktreeAutoCleanup,
    DeleteBranchOnCleanup,
    WorkspacePathTemplate,
    InitSubmodules,
    // Sandbox
    SandboxEnabledByDefault,
    YoloModeDefault,
    DefaultImage,
    Environment,
    SandboxAutoCleanup,
    CpuLimit,
    MemoryLimit,
    DefaultTerminalMode,
    ExtraVolumes,
    PortMappings,
    VolumeIgnores,
    MountSsh,
    CustomInstruction,
    ContainerRuntime,
    // Tmux
    StatusBar,
    Mouse,
    Clipboard,
    // Session
    DefaultTool,
    StrictHotkeys,
    AgentExtraArgs,
    AgentCommandOverride,
    AgentStatusHooks,
    CustomAgents,
    AgentDetectAs,
    HostEnvironment,
    // Sound
    SoundEnabled,
    SoundMode,
    SoundVolume,
    SoundOnStart,
    SoundOnRunning,
    SoundOnWaiting,
    SoundOnIdle,
    SoundOnError,
    SoundOnApproval,
    // Status hooks
    StatusHooksEnabled,
    StatusHookDebounceMs,
    StatusHookOnStarting,
    StatusHookOnRunning,
    StatusHookOnWaiting,
    StatusHookOnIdle,
    StatusHookOnError,
    StatusHookOnChange,
    // Hooks
    HookOnCreate,
    HookOnLaunch,
    HookOnDestroy,
    // Web
    WebNotificationsEnabled,
    WebNotifyOnWaiting,
    WebNotifyOnIdle,
    WebNotifyOnError,
    WebNotifyOnWakeFire,
    // Cockpit (gated on the `serve` feature; the variants are always
    // present in the enum so external callers don't have to cfg-gate
    // their match arms)
    CockpitEnabled,
    CockpitDefaultForClaude,
    CockpitDefaultAgent,
    CockpitMaxConcurrentWorkers,
    CockpitReplayEvents,
    CockpitReplayBytes,
    CockpitNodePath,
    CockpitShowToolDurations,
    CockpitQueueDrainMode,
    CockpitMaxConcurrentResumes,
    CockpitForceEndTurnThresholdSecs,
    // Logging
    LoggingDefaultLevel,
    /// Per-target override; carries an index into `crate::logging::KNOWN_SUB_TARGETS`
    /// so the FieldKey enum stays `Copy` without carrying owned strings.
    LoggingTarget(u8),
    LoggingOutput,
    LoggingFilePath,
    LoggingRotation,
    LoggingMaxSizeMib,
    LoggingKeepCount,
    LoggingShowSpans,
}

/// Resolve a field value from global config and optional profile override.
/// Returns (value, has_override).
fn resolve_value<T: Clone>(scope: SettingsScope, global: T, profile: Option<T>) -> (T, bool) {
    match scope {
        SettingsScope::Global | SettingsScope::Repo => (global, false),
        SettingsScope::Profile => {
            let has_override = profile.is_some();
            let value = profile.unwrap_or(global);
            (value, has_override)
        }
    }
}

/// Resolve an optional field (Option<T>) where both global and profile values are Option<T>.
/// The `has_explicit_override` flag indicates if the profile explicitly set this field.
fn resolve_optional<T: Clone>(
    scope: SettingsScope,
    global: Option<T>,
    profile: Option<T>,
    has_explicit_override: bool,
) -> (Option<T>, bool) {
    match scope {
        SettingsScope::Global | SettingsScope::Repo => (global, false),
        SettingsScope::Profile => {
            let value = profile.or(global);
            (value, has_explicit_override)
        }
    }
}

/// Convert a FieldValue to a human-readable display string.
fn value_display_string(value: &FieldValue) -> String {
    match value {
        FieldValue::Bool(v) => if *v { "on" } else { "off" }.to_string(),
        FieldValue::Text(v) => {
            if v.is_empty() {
                "(empty)".to_string()
            } else {
                v.clone()
            }
        }
        FieldValue::Number(v) => v.to_string(),
        FieldValue::Select { selected, options } => {
            options.get(*selected).cloned().unwrap_or_default()
        }
        FieldValue::List(items) => format!("[{} items]", items.len()),
        FieldValue::OptionalText(v) => v.clone().unwrap_or_else(|| "(empty)".to_string()),
    }
}

/// Build the inherited display string when a field has an override.
fn inherited_if(has_override: bool, global_value: FieldValue) -> Option<String> {
    if has_override {
        Some(value_display_string(&global_value))
    } else {
        None
    }
}

/// Helper to set a profile override. Always stores the value; use 'r' key to clear overrides.
fn set_profile_override<T, S, F>(new_value: T, section: &mut Option<S>, set_field: F)
where
    T: Clone,
    S: Default,
    F: FnOnce(&mut S, Option<T>),
{
    let s = section.get_or_insert_with(S::default);
    set_field(s, Some(new_value));
}

/// Parse a list of "key=value" strings into a HashMap.
fn parse_key_value_list(items: &[String]) -> std::collections::HashMap<String, String> {
    items
        .iter()
        .filter_map(|item| {
            let (k, v) = item.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// Value types for settings fields
#[derive(Debug, Clone)]
pub enum FieldValue {
    Bool(bool),
    Text(String),
    Number(u64),
    Select {
        selected: usize,
        options: Vec<String>,
    },
    List(Vec<String>),
    OptionalText(Option<String>),
}

/// A setting field with metadata
#[derive(Debug, Clone)]
pub struct SettingField {
    pub key: FieldKey,
    pub label: &'static str,
    pub description: &'static str,
    pub value: FieldValue,
    pub category: SettingsCategory,
    /// Whether this field has a profile/repo override
    pub has_override: bool,
    /// Human-readable display of the inherited (global/base) value, set when has_override is true
    pub inherited_display: Option<String>,
}

impl SettingField {
    pub fn validate(&self) -> Result<(), String> {
        match (&self.key, &self.value) {
            (FieldKey::CheckIntervalHours, FieldValue::Number(n)) => {
                validate_check_interval(*n)?;
                Ok(())
            }
            (FieldKey::MemoryLimit, FieldValue::OptionalText(Some(v))) => {
                crate::session::validate_memory_limit(v)?;
                Ok(())
            }
            // Sound field validation - check if sound file exists
            (
                FieldKey::SoundOnStart
                | FieldKey::SoundOnRunning
                | FieldKey::SoundOnWaiting
                | FieldKey::SoundOnIdle
                | FieldKey::SoundOnError
                | FieldKey::SoundOnApproval,
                FieldValue::OptionalText(Some(name)),
            ) => {
                if !name.is_empty() {
                    validate_sound_exists(name)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Build fields for a category based on scope and current config values.
///
/// For Repo scope, `global` should be the resolved (global+profile merged) config,
/// and `profile` should be the repo config converted to ProfileConfig via `repo_config_to_profile`.
pub fn build_fields_for_category(
    category: SettingsCategory,
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    match category {
        SettingsCategory::Theme => build_theme_fields(scope, global, profile),
        SettingsCategory::Updates => build_updates_fields(scope, global, profile),
        SettingsCategory::Worktree => build_worktree_fields(scope, global, profile),
        SettingsCategory::Sandbox => build_sandbox_fields(scope, global, profile),
        SettingsCategory::Tmux => build_tmux_fields(scope, global, profile),
        SettingsCategory::Session => build_session_fields(scope, global, profile),
        SettingsCategory::Sound => build_sound_fields(scope, global, profile),
        SettingsCategory::StatusHooks => build_status_hook_fields(scope, global, profile),
        SettingsCategory::Hooks => build_hooks_fields(scope, global, profile),
        SettingsCategory::Web => build_web_fields(scope, global, profile),
        SettingsCategory::Cockpit => build_cockpit_fields(scope, global, profile),
        SettingsCategory::Logging => build_logging_fields(global),
    }
}

const LOG_LEVEL_OPTIONS: &[&str] = &["trace", "debug", "info", "warn", "error"];
const LOG_LEVEL_OVERRIDE_OPTIONS: &[&str] =
    &["(default)", "trace", "debug", "info", "warn", "error"];
const SINK_OPTIONS: &[&str] = &["file", "stdout"];
const ROTATION_OPTIONS: &[&str] = &["size", "never"];

fn level_index(level: &str, opts: &[&str]) -> usize {
    opts.iter().position(|&o| o == level).unwrap_or(0)
}

fn build_logging_fields(global: &Config) -> Vec<SettingField> {
    let mut fields = Vec::with_capacity(1 + crate::logging::KNOWN_SUB_TARGETS.len());

    let default_idx = level_index(&global.logging.default_level, LOG_LEVEL_OPTIONS);
    fields.push(SettingField {
        key: FieldKey::LoggingDefaultLevel,
        label: "Default level",
        description: "Baseline applied to every known target root. Per-target overrides win.",
        value: FieldValue::Select {
            selected: default_idx,
            options: LOG_LEVEL_OPTIONS.iter().map(|s| s.to_string()).collect(),
        },
        category: SettingsCategory::Logging,
        has_override: false,
        inherited_display: None,
    });

    for (i, target) in crate::logging::KNOWN_SUB_TARGETS.iter().enumerate() {
        let current = global
            .logging
            .targets
            .get(*target)
            .map(|s| s.as_str())
            .unwrap_or("(default)");
        let idx = level_index(current, LOG_LEVEL_OVERRIDE_OPTIONS);
        fields.push(SettingField {
            key: FieldKey::LoggingTarget(i as u8),
            label: target,
            description: "Per-target override; (default) inherits the baseline.",
            value: FieldValue::Select {
                selected: idx,
                options: LOG_LEVEL_OVERRIDE_OPTIONS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            },
            category: SettingsCategory::Logging,
            has_override: false,
            inherited_display: None,
        });
    }

    // Sink-shape knobs. Changing any of these requires a process restart
    // (the tracing subscriber is a global singleton; the rotating writer
    // holds its policy + file handle for the life of the process).
    let output_idx = level_index(
        match global.logging.output {
            crate::session::config::SinkKind::File => "file",
            crate::session::config::SinkKind::Stdout => "stdout",
        },
        SINK_OPTIONS,
    );
    fields.push(SettingField {
        key: FieldKey::LoggingOutput,
        label: "Output (restart req.)",
        description:
            "Where tracing lands: file (default) or stdout. TUI / daemon child / runner coerce \
             to file regardless. Restart aoe for changes to take effect.",
        value: FieldValue::Select {
            selected: output_idx,
            options: SINK_OPTIONS.iter().map(|s| s.to_string()).collect(),
        },
        category: SettingsCategory::Logging,
        has_override: false,
        inherited_display: None,
    });
    fields.push(SettingField {
        key: FieldKey::LoggingFilePath,
        label: "File path (restart req.)",
        description: "Log file location. Relative paths resolve under the app data dir; absolute \
                      paths are used verbatim. Restart aoe for changes.",
        value: FieldValue::Text(global.logging.file_path.clone()),
        category: SettingsCategory::Logging,
        has_override: false,
        inherited_display: None,
    });

    let rotation_idx = level_index(
        match global.logging.rotation {
            crate::session::config::RotationKind::Size => "size",
            crate::session::config::RotationKind::Never => "never",
        },
        ROTATION_OPTIONS,
    );
    fields.push(SettingField {
        key: FieldKey::LoggingRotation,
        label: "Rotation (restart req.)",
        description: "size rotates when the live file crosses the threshold; never disables \
                      rotation. Restart aoe for changes.",
        value: FieldValue::Select {
            selected: rotation_idx,
            options: ROTATION_OPTIONS.iter().map(|s| s.to_string()).collect(),
        },
        category: SettingsCategory::Logging,
        has_override: false,
        inherited_display: None,
    });
    fields.push(SettingField {
        key: FieldKey::LoggingMaxSizeMib,
        label: "Max size MiB (restart req.)",
        description: "Rotation threshold in MiB. Ignored when rotation = never.",
        value: FieldValue::Number(global.logging.max_size_mib),
        category: SettingsCategory::Logging,
        has_override: false,
        inherited_display: None,
    });
    fields.push(SettingField {
        key: FieldKey::LoggingKeepCount,
        label: "Keep count (restart req.)",
        description: "How many rotated files to retain (.1 through .keep_count).",
        value: FieldValue::Number(global.logging.keep_count as u64),
        category: SettingsCategory::Logging,
        has_override: false,
        inherited_display: None,
    });
    fields.push(SettingField {
        key: FieldKey::LoggingShowSpans,
        label: "Show span context (restart req.)",
        description:
            "When on, every log line is prefixed with the names + fields of the spans wrapping it \
             (e.g. `http_request{request_id=... method=GET path=...}` from the per-request middleware). \
             Useful for grep-correlation across async boundaries when triaging; noisy on idle polling endpoints. \
             Off by default keeps the log readable.",
        value: FieldValue::Bool(global.logging.show_spans),
        category: SettingsCategory::Logging,
        has_override: false,
        inherited_display: None,
    });

    fields
}

fn build_cockpit_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let p = profile.cockpit.as_ref();

    let (enabled, enabled_override) =
        resolve_value(scope, global.cockpit.enabled, p.and_then(|c| c.enabled));
    let (default_for_claude, dfc_override) = resolve_value(
        scope,
        global.cockpit.default_for_claude,
        p.and_then(|c| c.default_for_claude),
    );
    let (default_agent, da_override) = resolve_value(
        scope,
        global.cockpit.default_agent.clone(),
        p.and_then(|c| c.default_agent.clone()),
    );
    let (max_workers, mw_override) = resolve_value(
        scope,
        global.cockpit.max_concurrent_workers,
        p.and_then(|c| c.max_concurrent_workers),
    );
    let (replay_events, re_override) = resolve_value(
        scope,
        global.cockpit.replay_events,
        p.and_then(|c| c.replay_events),
    );
    let (replay_bytes, rb_override) = resolve_value(
        scope,
        global.cockpit.replay_bytes,
        p.and_then(|c| c.replay_bytes),
    );
    let (node_path, np_override) = resolve_value(
        scope,
        global.cockpit.node_path.clone(),
        p.and_then(|c| c.node_path.clone()),
    );
    let (show_tool_durations, std_override) = resolve_value(
        scope,
        global.cockpit.show_tool_durations,
        p.and_then(|c| c.show_tool_durations),
    );
    let (queue_drain_mode, qdm_override) = resolve_value(
        scope,
        global.cockpit.queue_drain_mode,
        p.and_then(|c| c.queue_drain_mode),
    );
    let (max_concurrent_resumes, mcr_override) = resolve_value(
        scope,
        global.cockpit.max_concurrent_resumes,
        p.and_then(|c| c.max_concurrent_resumes),
    );
    let (force_end_turn_threshold_secs, fet_override) = resolve_value(
        scope,
        global.cockpit.force_end_turn_threshold_secs,
        p.and_then(|c| c.force_end_turn_threshold_secs),
    );

    vec![
        SettingField {
            key: FieldKey::CockpitEnabled,
            label: "Cockpit enabled",
            description: "Master switch for cockpit (ACP-based native agent rendering). When off, sessions use the terminal/PTY view even with --cockpit.",
            value: FieldValue::Bool(enabled),
            category: SettingsCategory::Cockpit,
            has_override: enabled_override,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::CockpitDefaultForClaude,
            label: "Default for Claude (mobile)",
            description: "On mobile clients, default new Claude sessions to cockpit mode.",
            value: FieldValue::Bool(default_for_claude),
            category: SettingsCategory::Cockpit,
            has_override: dfc_override,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::CockpitDefaultAgent,
            label: "Default agent",
            description: "Cockpit agent to use when --agent is not specified (e.g., aoe-agent, claude-code, gemini).",
            value: FieldValue::Text(default_agent),
            category: SettingsCategory::Cockpit,
            has_override: da_override,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::CockpitMaxConcurrentWorkers,
            label: "Max concurrent workers",
            description: "Hard cap on simultaneously running cockpit agent subprocesses; additional sessions queue.",
            value: FieldValue::Number(u64::from(max_workers)),
            category: SettingsCategory::Cockpit,
            has_override: mw_override,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::CockpitReplayEvents,
            label: "History cap (events)",
            description: "Per-session retention cap on cockpit events. 0 = unlimited (default); set a non-zero value to bound disk usage on long-running sessions.",
            value: FieldValue::Number(u64::from(replay_events)),
            category: SettingsCategory::Cockpit,
            has_override: re_override,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::CockpitReplayBytes,
            label: "Replay buffer bytes",
            description: "Maximum bytes of cockpit events kept in the per-session replay buffer.",
            value: FieldValue::Number(replay_bytes),
            category: SettingsCategory::Cockpit,
            has_override: rb_override,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::CockpitNodePath,
            label: "Node path",
            description: "Override Node.js binary location. Empty -> auto-resolve via AOE_COCKPIT_NODE / PATH / bundled.",
            value: FieldValue::Text(node_path),
            category: SettingsCategory::Cockpit,
            has_override: np_override,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::CockpitShowToolDurations,
            label: "Show tool-call durations",
            description: "Render an elapsed-time label on every cockpit tool card. Cross-device via config.toml. Note: the underlying measurement is currently imprecise on claude-agent-acp (no `status: in_progress` signal); durations include stream-arrival skew. Defaults to on; turn off if the inflated numbers are more confusing than useful.",
            value: FieldValue::Bool(show_tool_durations),
            category: SettingsCategory::Cockpit,
            has_override: std_override,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::CockpitMaxConcurrentResumes,
            label: "Max concurrent resumes",
            description: "Upper bound on parallel cockpit worker spawns/attaches the reconciler runs on `aoe serve` cold start. Default 4 keeps Node.js bootup memory within bounds for laptops/Pis (each claude-agent-acp is ~50-80 MB transient). Bounded at runtime by `min(this, max_concurrent_workers).max(1)`. See #1088.",
            value: FieldValue::Number(u64::from(max_concurrent_resumes)),
            category: SettingsCategory::Cockpit,
            has_override: mcr_override,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::CockpitQueueDrainMode,
            label: "Queue drain mode",
            description: "How the web composer dispatches follow-up prompts queued while the agent was busy. Combined (default) joins every queued entry with a blank line and sends them as a single prompt on the next Stopped; one response covers the whole batch. Serial fires one entry at a time; each gets its own response. See #1031.",
            value: FieldValue::Select {
                selected: match queue_drain_mode {
                    crate::session::config::QueueDrainMode::Combined => 0,
                    crate::session::config::QueueDrainMode::Serial => 1,
                },
                options: vec!["combined".to_string(), "serial".to_string()],
            },
            category: SettingsCategory::Cockpit,
            has_override: qdm_override,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::CockpitForceEndTurnThresholdSecs,
            label: "Force end turn threshold (s)",
            description: "Seconds of streaming inactivity after which the cockpit web UI offers a \"Force end turn\" button. When the spinner is stuck (a missed Stopped event), clicking the button clears the local spinner and asks the daemon to publish a synthetic Stopped + best-effort session/cancel. Default 30s. See #1100.",
            value: FieldValue::Number(u64::from(force_end_turn_threshold_secs)),
            category: SettingsCategory::Cockpit,
            has_override: fet_override,
            inherited_display: None,
        },
    ]
}

fn build_web_fields(
    scope: SettingsScope,
    global: &Config,
    _profile: &ProfileConfig,
) -> Vec<SettingField> {
    // Web settings are server-global, not profile-scoped. In Profile mode
    // we still surface the field (read-only) so users discover it; writes
    // always apply to the global config.
    let _ = scope;

    vec![
        SettingField {
            key: FieldKey::WebNotificationsEnabled,
            label: "Push notifications",
            description: "Allow the web dashboard to deliver browser push notifications (server-wide kill switch).",
            value: FieldValue::Bool(global.web.notifications_enabled),
            category: SettingsCategory::Web,
            has_override: false,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::WebNotifyOnWaiting,
            label: "Notify on waiting",
            description: "Default: send a push when a session transitions Running to Waiting (agent is asking for input). Sessions can override individually.",
            value: FieldValue::Bool(global.web.notify_on_waiting),
            category: SettingsCategory::Web,
            has_override: false,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::WebNotifyOnIdle,
            label: "Notify on idle",
            description: "Default: send a push when a session finishes (Running to Idle). Off by default because short sessions make this noisy; sessions can opt in individually.",
            value: FieldValue::Bool(global.web.notify_on_idle),
            category: SettingsCategory::Web,
            has_override: false,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::WebNotifyOnError,
            label: "Notify on error",
            description: "Default: send a push when a session errors (Running to Error).",
            value: FieldValue::Bool(global.web.notify_on_error),
            category: SettingsCategory::Web,
            has_override: false,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::WebNotifyOnWakeFire,
            label: "Notify on scheduled wake",
            description: "Default: send a push when a cockpit session's ScheduleWakeup timer fires (the next /loop turn starts). Suppressed if the TUI or web dashboard has been active in the last 30s.",
            value: FieldValue::Bool(global.web.notify_on_wake_fire),
            category: SettingsCategory::Web,
            has_override: false,
            inherited_display: None,
        },
    ]
}

fn build_theme_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let theme = profile.theme.as_ref();

    let (name, has_override) = resolve_value(
        scope,
        global.theme.name.clone(),
        theme.and_then(|t| t.name.clone()),
    );

    let options: Vec<String> = available_themes();
    let selected = options.iter().position(|s| s == &name).unwrap_or(0);

    let global_selected = options
        .iter()
        .position(|s| s == &global.theme.name)
        .unwrap_or(0);
    let inherited = inherited_if(
        has_override,
        FieldValue::Select {
            selected: global_selected,
            options: options.clone(),
        },
    );

    let color_mode_options: Vec<String> = vec!["truecolor".to_string(), "palette".to_string()];
    let (color_mode_val, cm_has_override) = resolve_value(
        scope,
        global.theme.color_mode.clone(),
        theme.and_then(|t| t.color_mode.clone()),
    );
    let cm_selected = match color_mode_val {
        crate::session::config::ColorMode::Truecolor => 0,
        crate::session::config::ColorMode::Palette => 1,
    };
    let global_cm_selected = match global.theme.color_mode {
        crate::session::config::ColorMode::Truecolor => 0,
        crate::session::config::ColorMode::Palette => 1,
    };
    let cm_inherited = inherited_if(
        cm_has_override,
        FieldValue::Select {
            selected: global_cm_selected,
            options: color_mode_options.clone(),
        },
    );

    let (idle_decay_minutes, idle_decay_override) = resolve_value(
        scope,
        global.theme.idle_decay_minutes,
        theme.and_then(|t| t.idle_decay_minutes),
    );

    let mut fields = Vec::with_capacity(4);
    // The profile description is a profile-only field (no global counterpart),
    // so we surface it only when the user is editing the Profile scope. It
    // sits at the top of the Theme category so it is the first thing they
    // see when looking at a profile's settings.
    if scope == SettingsScope::Profile {
        fields.push(SettingField {
            key: FieldKey::ProfileDescription,
            label: "Description",
            description:
                "Short, human-readable description of what this profile does. Shown as helper \
                 text under the profile name in the new-session picker (TUI + web).",
            value: FieldValue::OptionalText(profile.description.clone()),
            category: SettingsCategory::Theme,
            has_override: profile.description.is_some(),
            inherited_display: None,
        });
    }
    fields.extend([
        SettingField {
            key: FieldKey::ThemeName,
            label: "Theme",
            description: "Color theme for the TUI",
            value: FieldValue::Select { selected, options },
            category: SettingsCategory::Theme,
            has_override,
            inherited_display: inherited,
        },
        SettingField {
            key: FieldKey::ThemeColorMode,
            label: "Color Mode",
            description: "Truecolor (24-bit RGB) or palette (xterm-256). Use palette if your terminal mangles RGB escapes.",
            value: FieldValue::Select {
                selected: cm_selected,
                options: color_mode_options,
            },
            category: SettingsCategory::Theme,
            has_override: cm_has_override,
            inherited_display: cm_inherited,
        },
        SettingField {
            key: FieldKey::IdleDecayMinutes,
            label: "Idle Decay (minutes)",
            description: "Off by default (0). Set a positive value to opt in: a freshly-stopped Idle session keeps a fresh-idle tint and an animated breathe icon for this many minutes before snapping back to the static look, and is treated as actionable by the `w` keybind. The time-since-stop column on Idle rows shows regardless of this setting.",
            value: FieldValue::Number(idle_decay_minutes),
            category: SettingsCategory::Theme,
            has_override: idle_decay_override,
            inherited_display: inherited_if(
                idle_decay_override,
                FieldValue::Number(global.theme.idle_decay_minutes),
            ),
        },
    ]);
    fields
}

fn build_updates_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let updates = profile.updates.as_ref();

    let (check_enabled, o1) = resolve_value(
        scope,
        global.updates.check_enabled,
        updates.and_then(|u| u.check_enabled),
    );
    let (check_interval, o2) = resolve_value(
        scope,
        global.updates.check_interval_hours,
        updates.and_then(|u| u.check_interval_hours),
    );
    let (notify_in_cli, o3) = resolve_value(
        scope,
        global.updates.notify_in_cli,
        updates.and_then(|u| u.notify_in_cli),
    );
    let (web_poll, o4) = resolve_value(
        scope,
        global.updates.web_poll_interval_minutes,
        updates.and_then(|u| u.web_poll_interval_minutes),
    );

    vec![
        SettingField {
            key: FieldKey::CheckEnabled,
            label: "Check for Updates",
            description: "Automatically check for updates on startup",
            value: FieldValue::Bool(check_enabled),
            category: SettingsCategory::Updates,
            has_override: o1,
            inherited_display: inherited_if(o1, FieldValue::Bool(global.updates.check_enabled)),
        },
        SettingField {
            key: FieldKey::CheckIntervalHours,
            label: "Check Interval (hours)",
            description: "How often to check for updates",
            value: FieldValue::Number(check_interval),
            category: SettingsCategory::Updates,
            has_override: o2,
            inherited_display: inherited_if(
                o2,
                FieldValue::Number(global.updates.check_interval_hours),
            ),
        },
        SettingField {
            key: FieldKey::NotifyInCli,
            label: "Notify in CLI",
            description: "Show update notifications in CLI output",
            value: FieldValue::Bool(notify_in_cli),
            category: SettingsCategory::Updates,
            has_override: o3,
            inherited_display: inherited_if(o3, FieldValue::Bool(global.updates.notify_in_cli)),
        },
        SettingField {
            key: FieldKey::WebPollIntervalMinutes,
            label: "Web Poll Interval (minutes)",
            description: "How often the web dashboard re-polls for new releases",
            value: FieldValue::Number(web_poll),
            category: SettingsCategory::Updates,
            has_override: o4,
            inherited_display: inherited_if(
                o4,
                FieldValue::Number(global.updates.web_poll_interval_minutes),
            ),
        },
    ]
}

fn build_worktree_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let wt = profile.worktree.as_ref();

    let (enabled, o0) = resolve_value(scope, global.worktree.enabled, wt.and_then(|w| w.enabled));
    let (path_template, o1) = resolve_value(
        scope,
        global.worktree.path_template.clone(),
        wt.and_then(|w| w.path_template.clone()),
    );
    let (bare_repo_template, o2) = resolve_value(
        scope,
        global.worktree.bare_repo_path_template.clone(),
        wt.and_then(|w| w.bare_repo_path_template.clone()),
    );
    let (auto_cleanup, o3) = resolve_value(
        scope,
        global.worktree.auto_cleanup,
        wt.and_then(|w| w.auto_cleanup),
    );
    let (delete_branch_on_cleanup, o4) = resolve_value(
        scope,
        global.worktree.delete_branch_on_cleanup,
        wt.and_then(|w| w.delete_branch_on_cleanup),
    );
    let (workspace_path_template, o5) = resolve_value(
        scope,
        global.worktree.workspace_path_template.clone(),
        wt.and_then(|w| w.workspace_path_template.clone()),
    );
    let (init_submodules, o6) = resolve_value(
        scope,
        global.worktree.init_submodules,
        wt.and_then(|w| w.init_submodules),
    );

    vec![
        SettingField {
            key: FieldKey::WorktreeEnabled,
            label: "Enabled by Default",
            description: "Enable worktree mode by default for new sessions",
            value: FieldValue::Bool(enabled),
            category: SettingsCategory::Worktree,
            has_override: o0,
            inherited_display: inherited_if(o0, FieldValue::Bool(global.worktree.enabled)),
        },
        SettingField {
            key: FieldKey::PathTemplate,
            label: "Path Template",
            description: "Template for worktree paths ({repo-name}, {branch})",
            value: FieldValue::Text(path_template),
            category: SettingsCategory::Worktree,
            has_override: o1,
            inherited_display: inherited_if(
                o1,
                FieldValue::Text(global.worktree.path_template.clone()),
            ),
        },
        SettingField {
            key: FieldKey::BareRepoPathTemplate,
            label: "Bare Repo Template",
            description: "Template for bare repo worktree paths",
            value: FieldValue::Text(bare_repo_template),
            category: SettingsCategory::Worktree,
            has_override: o2,
            inherited_display: inherited_if(
                o2,
                FieldValue::Text(global.worktree.bare_repo_path_template.clone()),
            ),
        },
        SettingField {
            key: FieldKey::WorktreeAutoCleanup,
            label: "Auto Cleanup",
            description: "Automatically clean up worktrees on session delete",
            value: FieldValue::Bool(auto_cleanup),
            category: SettingsCategory::Worktree,
            has_override: o3,
            inherited_display: inherited_if(o3, FieldValue::Bool(global.worktree.auto_cleanup)),
        },
        SettingField {
            key: FieldKey::DeleteBranchOnCleanup,
            label: "Delete Branch on Cleanup",
            description: "Also delete the git branch when deleting a worktree",
            value: FieldValue::Bool(delete_branch_on_cleanup),
            category: SettingsCategory::Worktree,
            has_override: o4,
            inherited_display: inherited_if(
                o4,
                FieldValue::Bool(global.worktree.delete_branch_on_cleanup),
            ),
        },
        SettingField {
            key: FieldKey::WorkspacePathTemplate,
            label: "Workspace Path Template",
            description: "Template for multi-repo workspace directories ({branch}, {session-id})",
            value: FieldValue::Text(workspace_path_template),
            category: SettingsCategory::Worktree,
            has_override: o5,
            inherited_display: inherited_if(
                o5,
                FieldValue::Text(global.worktree.workspace_path_template.clone()),
            ),
        },
        SettingField {
            key: FieldKey::InitSubmodules,
            label: "Init Submodules",
            description: "Run `git submodule update --init --recursive` after creating a worktree",
            value: FieldValue::Bool(init_submodules),
            category: SettingsCategory::Worktree,
            has_override: o6,
            inherited_display: inherited_if(o6, FieldValue::Bool(global.worktree.init_submodules)),
        },
    ]
}

fn build_sandbox_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let sb = profile.sandbox.as_ref();

    let (enabled_by_default, o1) = resolve_value(
        scope,
        global.sandbox.enabled_by_default,
        sb.and_then(|s| s.enabled_by_default),
    );
    let (default_image, o3) = resolve_value(
        scope,
        global.sandbox.default_image.clone(),
        sb.and_then(|s| s.default_image.clone()),
    );
    let (environment, o4) = resolve_value(
        scope,
        global.sandbox.environment.clone(),
        sb.and_then(|s| s.environment.clone()),
    );
    let (auto_cleanup, o5) = resolve_value(
        scope,
        global.sandbox.auto_cleanup,
        sb.and_then(|s| s.auto_cleanup),
    );
    let (cpu_limit, o_cpu) = resolve_optional(
        scope,
        global.sandbox.cpu_limit.clone(),
        sb.and_then(|s| s.cpu_limit.clone()),
        sb.map(|s| s.cpu_limit.is_some()).unwrap_or(false),
    );
    let (memory_limit, o_mem) = resolve_optional(
        scope,
        global.sandbox.memory_limit.clone(),
        sb.and_then(|s| s.memory_limit.clone()),
        sb.map(|s| s.memory_limit.is_some()).unwrap_or(false),
    );
    let (default_terminal_mode, o6) = resolve_value(
        scope,
        global.sandbox.default_terminal_mode,
        sb.and_then(|s| s.default_terminal_mode),
    );
    let (extra_volumes, o_ev) = resolve_value(
        scope,
        global.sandbox.extra_volumes.clone(),
        sb.and_then(|s| s.extra_volumes.clone()),
    );
    let (port_mappings, o_pm) = resolve_value(
        scope,
        global.sandbox.port_mappings.clone(),
        sb.and_then(|s| s.port_mappings.clone()),
    );
    let (volume_ignores, o7) = resolve_value(
        scope,
        global.sandbox.volume_ignores.clone(),
        sb.and_then(|s| s.volume_ignores.clone()),
    );
    let (mount_ssh, o8) = resolve_value(
        scope,
        global.sandbox.mount_ssh,
        sb.and_then(|s| s.mount_ssh),
    );
    let (custom_instruction, o_ci) = resolve_optional(
        scope,
        global.sandbox.custom_instruction.clone(),
        sb.and_then(|s| s.custom_instruction.clone()),
        sb.map(|s| s.custom_instruction.is_some()).unwrap_or(false),
    );
    let (container_runtime, o_cr) = resolve_value(
        scope,
        global.sandbox.container_runtime,
        sb.and_then(|s| s.container_runtime),
    );

    let terminal_mode_selected = match default_terminal_mode {
        DefaultTerminalMode::Host => 0,
        DefaultTerminalMode::Container => 1,
    };

    let container_runtime_selected = match container_runtime {
        ContainerRuntimeName::Docker => 0,
        ContainerRuntimeName::Podman => 1,
        ContainerRuntimeName::AppleContainer => 2,
    };

    let global_terminal_mode_selected = match global.sandbox.default_terminal_mode {
        DefaultTerminalMode::Host => 0,
        DefaultTerminalMode::Container => 1,
    };
    let terminal_mode_options = vec!["Host".into(), "Container".into()];

    let global_container_runtime_selected = match global.sandbox.container_runtime {
        ContainerRuntimeName::Docker => 0,
        ContainerRuntimeName::Podman => 1,
        ContainerRuntimeName::AppleContainer => 2,
    };
    let container_runtime_options =
        vec!["Docker".into(), "Podman".into(), "Apple Container".into()];

    vec![
        SettingField {
            key: FieldKey::SandboxEnabledByDefault,
            label: "Enabled by Default",
            description: "Enable sandbox mode by default for new sessions",
            value: FieldValue::Bool(enabled_by_default),
            category: SettingsCategory::Sandbox,
            has_override: o1,
            inherited_display: inherited_if(
                o1,
                FieldValue::Bool(global.sandbox.enabled_by_default),
            ),
        },
        SettingField {
            key: FieldKey::DefaultImage,
            label: "Default Image",
            description: "Container image to use for sandboxes",
            value: FieldValue::Text(default_image),
            category: SettingsCategory::Sandbox,
            has_override: o3,
            inherited_display: inherited_if(
                o3,
                FieldValue::Text(global.sandbox.default_image.clone()),
            ),
        },
        SettingField {
            key: FieldKey::Environment,
            label: "Sandbox Environment",
            description: "Env vars injected into the container: KEY=value (literal, appears in argv), KEY=$VAR (passthrough from host, hidden from argv), KEY=$$literal (escape a leading $), or bare KEY (passthrough). For host (non-sandboxed) sessions, see Session > Host Environment instead.",
            value: FieldValue::List(environment),
            category: SettingsCategory::Sandbox,
            has_override: o4,
            inherited_display: inherited_if(
                o4,
                FieldValue::List(global.sandbox.environment.clone()),
            ),
        },
        SettingField {
            key: FieldKey::SandboxAutoCleanup,
            label: "Auto Cleanup",
            description: "Remove containers when sessions are deleted",
            value: FieldValue::Bool(auto_cleanup),
            category: SettingsCategory::Sandbox,
            has_override: o5,
            inherited_display: inherited_if(o5, FieldValue::Bool(global.sandbox.auto_cleanup)),
        },
        SettingField {
            key: FieldKey::CpuLimit,
            label: "CPU Limit",
            description: "CPU limit for containers (e.g. \"4\")",
            value: FieldValue::OptionalText(cpu_limit),
            category: SettingsCategory::Sandbox,
            has_override: o_cpu,
            inherited_display: inherited_if(
                o_cpu,
                FieldValue::OptionalText(global.sandbox.cpu_limit.clone()),
            ),
        },
        SettingField {
            key: FieldKey::MemoryLimit,
            label: "Memory Limit",
            description: "Memory limit for containers (e.g. \"8g\", \"512m\")",
            value: FieldValue::OptionalText(memory_limit),
            category: SettingsCategory::Sandbox,
            has_override: o_mem,
            inherited_display: inherited_if(
                o_mem,
                FieldValue::OptionalText(global.sandbox.memory_limit.clone()),
            ),
        },
        SettingField {
            key: FieldKey::DefaultTerminalMode,
            label: "Default Terminal Mode",
            description: "Default terminal for sandboxed sessions (toggle with 'c' key)",
            value: FieldValue::Select {
                selected: terminal_mode_selected,
                options: terminal_mode_options.clone(),
            },
            category: SettingsCategory::Sandbox,
            has_override: o6,
            inherited_display: inherited_if(
                o6,
                FieldValue::Select {
                    selected: global_terminal_mode_selected,
                    options: terminal_mode_options,
                },
            ),
        },
        SettingField {
            key: FieldKey::ExtraVolumes,
            label: "Extra Volumes",
            description: "Additional volume mounts (host:container or host:container:ro)",
            value: FieldValue::List(extra_volumes),
            category: SettingsCategory::Sandbox,
            has_override: o_ev,
            inherited_display: inherited_if(
                o_ev,
                FieldValue::List(global.sandbox.extra_volumes.clone()),
            ),
        },
        SettingField {
            key: FieldKey::PortMappings,
            label: "Port Mappings",
            description: "Expose container ports to host (e.g. 3000:3000)",
            value: FieldValue::List(port_mappings),
            category: SettingsCategory::Sandbox,
            has_override: o_pm,
            inherited_display: inherited_if(
                o_pm,
                FieldValue::List(global.sandbox.port_mappings.clone()),
            ),
        },
        SettingField {
            key: FieldKey::VolumeIgnores,
            label: "Volume Ignores",
            description: "Directories to exclude from host mount (e.g. target, node_modules)",
            value: FieldValue::List(volume_ignores),
            category: SettingsCategory::Sandbox,
            has_override: o7,
            inherited_display: inherited_if(
                o7,
                FieldValue::List(global.sandbox.volume_ignores.clone()),
            ),
        },
        SettingField {
            key: FieldKey::MountSsh,
            label: "Mount SSH",
            description: "Mount ~/.ssh into sandbox containers (for git SSH access)",
            value: FieldValue::Bool(mount_ssh),
            category: SettingsCategory::Sandbox,
            has_override: o8,
            inherited_display: inherited_if(o8, FieldValue::Bool(global.sandbox.mount_ssh)),
        },
        SettingField {
            key: FieldKey::CustomInstruction,
            label: "Custom Instruction",
            description: "Custom instruction text appended to the agent's system prompt in sandboxed sessions (Claude, Codex only)",
            value: FieldValue::OptionalText(custom_instruction),
            category: SettingsCategory::Sandbox,
            has_override: o_ci,
            inherited_display: inherited_if(
                o_ci,
                FieldValue::OptionalText(global.sandbox.custom_instruction.clone()),
            ),
        },
        SettingField {
            key: FieldKey::ContainerRuntime,
            label: "Container Runtime",
            description: "Container runtime for sandboxing",
            value: FieldValue::Select {
                selected: container_runtime_selected,
                options: container_runtime_options.clone(),
            },
            category: SettingsCategory::Sandbox,
            has_override: o_cr,
            inherited_display: inherited_if(
                o_cr,
                FieldValue::Select {
                    selected: global_container_runtime_selected,
                    options: container_runtime_options,
                },
            ),
        },
    ]
}

fn build_tmux_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let tmux = profile.tmux.as_ref();

    let (status_bar, status_bar_override) = resolve_value(
        scope,
        global.tmux.status_bar,
        tmux.and_then(|t| t.status_bar),
    );

    let (mouse, mouse_override) =
        resolve_value(scope, global.tmux.mouse, tmux.and_then(|t| t.mouse));

    let (clipboard, clipboard_override) =
        resolve_value(scope, global.tmux.clipboard, tmux.and_then(|t| t.clipboard));

    let status_bar_selected = match status_bar {
        TmuxStatusBarMode::Auto => 0,
        TmuxStatusBarMode::Enabled => 1,
        TmuxStatusBarMode::Disabled => 2,
    };

    let mouse_selected = match mouse {
        TmuxMouseMode::Auto => 0,
        TmuxMouseMode::Enabled => 1,
        TmuxMouseMode::Disabled => 2,
    };

    let clipboard_selected = match clipboard {
        TmuxClipboardMode::Auto => 0,
        TmuxClipboardMode::Enabled => 1,
        TmuxClipboardMode::Disabled => 2,
    };

    let global_status_bar_selected = match global.tmux.status_bar {
        TmuxStatusBarMode::Auto => 0,
        TmuxStatusBarMode::Enabled => 1,
        TmuxStatusBarMode::Disabled => 2,
    };
    let global_mouse_selected = match global.tmux.mouse {
        TmuxMouseMode::Auto => 0,
        TmuxMouseMode::Enabled => 1,
        TmuxMouseMode::Disabled => 2,
    };
    let global_clipboard_selected = match global.tmux.clipboard {
        TmuxClipboardMode::Auto => 0,
        TmuxClipboardMode::Enabled => 1,
        TmuxClipboardMode::Disabled => 2,
    };
    let tmux_options = vec!["Auto".into(), "Enabled".into(), "Disabled".into()];

    vec![
        SettingField {
            key: FieldKey::StatusBar,
            label: "Status Bar",
            description: "Control tmux status bar styling (Auto respects your tmux config)",
            value: FieldValue::Select {
                selected: status_bar_selected,
                options: tmux_options.clone(),
            },
            category: SettingsCategory::Tmux,
            has_override: status_bar_override,
            inherited_display: inherited_if(
                status_bar_override,
                FieldValue::Select {
                    selected: global_status_bar_selected,
                    options: tmux_options.clone(),
                },
            ),
        },
        SettingField {
            key: FieldKey::Mouse,
            label: "Mouse Support",
            description: "Control mouse scrolling (Auto respects your tmux config)",
            value: FieldValue::Select {
                selected: mouse_selected,
                options: tmux_options.clone(),
            },
            category: SettingsCategory::Tmux,
            has_override: mouse_override,
            inherited_display: inherited_if(
                mouse_override,
                FieldValue::Select {
                    selected: global_mouse_selected,
                    options: tmux_options.clone(),
                },
            ),
        },
        SettingField {
            key: FieldKey::Clipboard,
            label: "Clipboard Pass-through",
            description: "Forward OSC 52 clipboard from agents to your terminal (Auto respects your tmux config)",
            value: FieldValue::Select {
                selected: clipboard_selected,
                options: tmux_options.clone(),
            },
            category: SettingsCategory::Tmux,
            has_override: clipboard_override,
            inherited_display: inherited_if(
                clipboard_override,
                FieldValue::Select {
                    selected: global_clipboard_selected,
                    options: tmux_options,
                },
            ),
        },
    ]
}

fn build_session_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let session = profile.session.as_ref();

    let (default_tool, has_override) = resolve_optional(
        scope,
        global.session.default_tool.clone(),
        session.and_then(|s| s.default_tool.clone()),
        session.map(|s| s.default_tool.is_some()).unwrap_or(false),
    );

    let selected = crate::agents::settings_index_from_name(default_tool.as_deref());

    let mut options = vec!["Auto (first available)".to_string()];
    options.extend(crate::agents::agent_names().iter().map(|n| n.to_string()));

    let (yolo_mode_default, yolo_override) = resolve_value(
        scope,
        global.session.yolo_mode_default,
        session.and_then(|s| s.yolo_mode_default),
    );

    let (strict_hotkeys, strict_hotkeys_override) = resolve_value(
        scope,
        global.session.strict_hotkeys,
        session.and_then(|s| s.strict_hotkeys),
    );

    let (agent_status_hooks, status_hooks_override) = resolve_value(
        scope,
        global.session.agent_status_hooks,
        session.and_then(|s| s.agent_status_hooks),
    );

    let (host_environment, host_env_override) = resolve_value(
        scope,
        global.environment.clone(),
        profile.environment.clone(),
    );

    // Agent extra args: HashMap -> Vec<String> of "key=value" items for List field
    let (extra_args_map, extra_args_override) = resolve_value(
        scope,
        global.session.agent_extra_args.clone(),
        session.and_then(|s| s.agent_extra_args.clone()),
    );
    let extra_args_list: Vec<String> = {
        let mut items: Vec<_> = extra_args_map
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };

    // Agent command override: HashMap -> Vec<String> of "key=value" items
    let (cmd_override_map, cmd_override_override) = resolve_value(
        scope,
        global.session.agent_command_override.clone(),
        session.and_then(|s| s.agent_command_override.clone()),
    );
    let cmd_override_list: Vec<String> = {
        let mut items: Vec<_> = cmd_override_map
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };

    let global_tool_selected =
        crate::agents::settings_index_from_name(global.session.default_tool.as_deref());

    let global_extra_args_list: Vec<String> = {
        let mut items: Vec<_> = global
            .session
            .agent_extra_args
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };
    let global_cmd_override_list: Vec<String> = {
        let mut items: Vec<_> = global
            .session
            .agent_command_override
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };

    // Custom agents: HashMap -> Vec<String> of "name=command" items
    let (custom_agents_map, custom_agents_override) = resolve_value(
        scope,
        global.session.custom_agents.clone(),
        session.and_then(|s| s.custom_agents.clone()),
    );
    let custom_agents_list: Vec<String> = {
        let mut items: Vec<_> = custom_agents_map
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };
    let global_custom_agents_list: Vec<String> = {
        let mut items: Vec<_> = global
            .session
            .custom_agents
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };

    // Agent detect_as: HashMap -> Vec<String> of "name=builtin" items
    let (detect_as_map, detect_as_override) = resolve_value(
        scope,
        global.session.agent_detect_as.clone(),
        session.and_then(|s| s.agent_detect_as.clone()),
    );
    let detect_as_list: Vec<String> = {
        let mut items: Vec<_> = detect_as_map
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };
    let global_detect_as_list: Vec<String> = {
        let mut items: Vec<_> = global
            .session
            .agent_detect_as
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };

    vec![
        SettingField {
            key: FieldKey::DefaultTool,
            label: "Default Tool",
            description: "Default coding tool for new sessions",
            value: FieldValue::Select {
                selected,
                options: options.clone(),
            },
            category: SettingsCategory::Session,
            has_override,
            inherited_display: inherited_if(
                has_override,
                FieldValue::Select {
                    selected: global_tool_selected,
                    options,
                },
            ),
        },
        SettingField {
            key: FieldKey::YoloModeDefault,
            label: "YOLO Mode Default",
            description: "Enable YOLO mode by default for new sessions",
            value: FieldValue::Bool(yolo_mode_default),
            category: SettingsCategory::Session,
            has_override: yolo_override,
            inherited_display: inherited_if(
                yolo_override,
                FieldValue::Bool(global.session.yolo_mode_default),
            ),
        },
        SettingField {
            key: FieldKey::StrictHotkeys,
            label: "Strict Hotkeys",
            description:
                "Require Shift/Ctrl for action hotkeys (guards against dictation/stray input)",
            value: FieldValue::Bool(strict_hotkeys),
            category: SettingsCategory::Session,
            has_override: strict_hotkeys_override,
            inherited_display: inherited_if(
                strict_hotkeys_override,
                FieldValue::Bool(global.session.strict_hotkeys),
            ),
        },
        SettingField {
            key: FieldKey::AgentExtraArgs,
            label: "Agent Extra Args",
            description:
                "Per-agent extra arguments appended after the binary (e.g. opencode=--port 8080)",
            value: FieldValue::List(extra_args_list),
            category: SettingsCategory::Session,
            has_override: extra_args_override,
            inherited_display: inherited_if(
                extra_args_override,
                FieldValue::List(global_extra_args_list),
            ),
        },
        SettingField {
            key: FieldKey::AgentCommandOverride,
            label: "Agent Command Override",
            description: "Per-agent command override replacing the binary (e.g. claude=my-wrapper)",
            value: FieldValue::List(cmd_override_list),
            category: SettingsCategory::Session,
            has_override: cmd_override_override,
            inherited_display: inherited_if(
                cmd_override_override,
                FieldValue::List(global_cmd_override_list),
            ),
        },
        SettingField {
            key: FieldKey::CustomAgents,
            label: "Custom Agents",
            description:
                "User-defined agents: name=command (e.g. lenovo-claude=ssh -t lenovo claude)",
            value: FieldValue::List(custom_agents_list),
            category: SettingsCategory::Session,
            has_override: custom_agents_override,
            inherited_display: inherited_if(
                custom_agents_override,
                FieldValue::List(global_custom_agents_list),
            ),
        },
        SettingField {
            key: FieldKey::AgentDetectAs,
            label: "Agent Detect As",
            description: "Status detection mapping: agent=builtin (e.g. lenovo-claude=claude)",
            value: FieldValue::List(detect_as_list),
            category: SettingsCategory::Session,
            has_override: detect_as_override,
            inherited_display: inherited_if(
                detect_as_override,
                FieldValue::List(global_detect_as_list),
            ),
        },
        SettingField {
            key: FieldKey::AgentStatusHooks,
            label: "Agent Status Hooks",
            description: "Install status-detection hooks into the agent's config file",
            value: FieldValue::Bool(agent_status_hooks),
            category: SettingsCategory::Session,
            has_override: status_hooks_override,
            inherited_display: inherited_if(
                status_hooks_override,
                FieldValue::Bool(global.session.agent_status_hooks),
            ),
        },
        SettingField {
            key: FieldKey::HostEnvironment,
            label: "Host Environment",
            description: "Env vars injected into the host command line: KEY=value (literal), KEY=$VAR (passthrough from host), KEY=$$literal (escape a leading $), or bare KEY (passthrough). All forms resolve to a literal `KEY=value` arg in the spawned process, visible in `ps`; for secrets you want hidden from argv, configure Sandbox > Sandbox Environment instead. Profile value replaces the global list.",
            value: FieldValue::List(host_environment),
            category: SettingsCategory::Session,
            has_override: host_env_override,
            inherited_display: inherited_if(
                host_env_override,
                FieldValue::List(global.environment.clone()),
            ),
        },
    ]
}

fn build_sound_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let snd = profile.sound.as_ref();

    let (enabled, o1) = resolve_value(scope, global.sound.enabled, snd.and_then(|s| s.enabled));

    let (mode, o2) = resolve_value(
        scope,
        global.sound.mode.clone(),
        snd.and_then(|s| s.mode.clone()),
    );

    let mode_selected = match &mode {
        SoundMode::Random => 0,
        SoundMode::Specific(_) => 1,
    };

    let (on_start, o3) = resolve_optional(
        scope,
        global.sound.on_start.clone(),
        snd.and_then(|s| s.on_start.clone()),
        snd.map(|s| s.on_start.is_some()).unwrap_or(false),
    );
    let (on_running, o4) = resolve_optional(
        scope,
        global.sound.on_running.clone(),
        snd.and_then(|s| s.on_running.clone()),
        snd.map(|s| s.on_running.is_some()).unwrap_or(false),
    );
    let (on_waiting, o5) = resolve_optional(
        scope,
        global.sound.on_waiting.clone(),
        snd.and_then(|s| s.on_waiting.clone()),
        snd.map(|s| s.on_waiting.is_some()).unwrap_or(false),
    );
    let (on_idle, o6) = resolve_optional(
        scope,
        global.sound.on_idle.clone(),
        snd.and_then(|s| s.on_idle.clone()),
        snd.map(|s| s.on_idle.is_some()).unwrap_or(false),
    );
    let (on_error, o7) = resolve_optional(
        scope,
        global.sound.on_error.clone(),
        snd.and_then(|s| s.on_error.clone()),
        snd.map(|s| s.on_error.is_some()).unwrap_or(false),
    );
    let (on_approval, o8) = resolve_optional(
        scope,
        global.sound.on_approval.clone(),
        snd.and_then(|s| s.on_approval.clone()),
        snd.map(|s| s.on_approval.is_some()).unwrap_or(false),
    );

    let global_mode_selected = match &global.sound.mode {
        SoundMode::Random => 0,
        SoundMode::Specific(_) => 1,
    };
    let sound_mode_options = vec!["Random".into(), "Specific".into()];

    let (volume, o_vol) = resolve_value(scope, global.sound.volume, snd.and_then(|s| s.volume));
    let vol_opts = volume_options();
    let vol_idx = volume_to_index(volume);

    vec![
        SettingField {
            key: FieldKey::SoundEnabled,
            label: "Enabled",
            description: "Play sounds on agent state transitions",
            value: FieldValue::Bool(enabled),
            category: SettingsCategory::Sound,
            has_override: o1,
            inherited_display: inherited_if(o1, FieldValue::Bool(global.sound.enabled)),
        },
        SettingField {
            key: FieldKey::SoundMode,
            label: "Mode",
            description: "How to select sounds (Random or Specific file name)",
            value: FieldValue::Select {
                selected: mode_selected,
                options: sound_mode_options.clone(),
            },
            category: SettingsCategory::Sound,
            has_override: o2,
            inherited_display: inherited_if(
                o2,
                FieldValue::Select {
                    selected: global_mode_selected,
                    options: sound_mode_options,
                },
            ),
        },
        SettingField {
            key: FieldKey::SoundVolume,
            label: "Volume",
            description: "Playback volume (0.1 = min, 1.0 = normal, 1.5 = max), step 0.1. Ignored when aplay is the Linux backend.",
            value: FieldValue::Select {
                selected: vol_idx,
                options: vol_opts.clone(),
            },
            category: SettingsCategory::Sound,
            has_override: o_vol,
            inherited_display: inherited_if(
                o_vol,
                FieldValue::Select {
                    selected: volume_to_index(global.sound.volume),
                    options: vol_opts,
                },
            ),
        },
        SettingField {
            key: FieldKey::SoundOnStart,
            label: "On Start",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_start),
            category: SettingsCategory::Sound,
            has_override: o3,
            inherited_display: inherited_if(
                o3,
                FieldValue::OptionalText(global.sound.on_start.clone()),
            ),
        },
        SettingField {
            key: FieldKey::SoundOnRunning,
            label: "On Running",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_running),
            category: SettingsCategory::Sound,
            has_override: o4,
            inherited_display: inherited_if(
                o4,
                FieldValue::OptionalText(global.sound.on_running.clone()),
            ),
        },
        SettingField {
            key: FieldKey::SoundOnWaiting,
            label: "On Waiting",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_waiting),
            category: SettingsCategory::Sound,
            has_override: o5,
            inherited_display: inherited_if(
                o5,
                FieldValue::OptionalText(global.sound.on_waiting.clone()),
            ),
        },
        SettingField {
            key: FieldKey::SoundOnIdle,
            label: "On Idle",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_idle),
            category: SettingsCategory::Sound,
            has_override: o6,
            inherited_display: inherited_if(
                o6,
                FieldValue::OptionalText(global.sound.on_idle.clone()),
            ),
        },
        SettingField {
            key: FieldKey::SoundOnError,
            label: "On Error",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_error),
            category: SettingsCategory::Sound,
            has_override: o7,
            inherited_display: inherited_if(
                o7,
                FieldValue::OptionalText(global.sound.on_error.clone()),
            ),
        },
        SettingField {
            key: FieldKey::SoundOnApproval,
            label: "On Approval",
            description: "Cockpit only. Played in the browser when a session needs permission. Specify file name with extension",
            value: FieldValue::OptionalText(on_approval),
            category: SettingsCategory::Sound,
            has_override: o8,
            inherited_display: inherited_if(
                o8,
                FieldValue::OptionalText(global.sound.on_approval.clone()),
            ),
        },
    ]
}

fn build_hooks_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let hooks = profile.hooks.as_ref();

    let (on_create, o1) = resolve_value(
        scope,
        global.hooks.on_create.clone(),
        hooks.and_then(|h| h.on_create.clone()),
    );
    let (on_launch, o2) = resolve_value(
        scope,
        global.hooks.on_launch.clone(),
        hooks.and_then(|h| h.on_launch.clone()),
    );
    let (on_destroy, o3) = resolve_value(
        scope,
        global.hooks.on_destroy.clone(),
        hooks.and_then(|h| h.on_destroy.clone()),
    );

    vec![
        SettingField {
            key: FieldKey::HookOnCreate,
            label: "On Create",
            description: "Commands run once when a session is first created. Runs inside sandbox when enabled.",
            value: FieldValue::List(on_create),
            category: SettingsCategory::Hooks,
            has_override: o1,
            inherited_display: inherited_if(
                o1,
                FieldValue::List(global.hooks.on_create.clone()),
            ),
        },
        SettingField {
            key: FieldKey::HookOnLaunch,
            label: "On Launch",
            description: "Commands run every time a session starts. Runs inside sandbox when enabled.",
            value: FieldValue::List(on_launch),
            category: SettingsCategory::Hooks,
            has_override: o2,
            inherited_display: inherited_if(
                o2,
                FieldValue::List(global.hooks.on_launch.clone()),
            ),
        },
        SettingField {
            key: FieldKey::HookOnDestroy,
            label: "On Destroy",
            description: "Commands run when a session is deleted, before cleanup. Use for teardown (e.g. docker-compose down).",
            value: FieldValue::List(on_destroy),
            category: SettingsCategory::Hooks,
            has_override: o3,
            inherited_display: inherited_if(
                o3,
                FieldValue::List(global.hooks.on_destroy.clone()),
            ),
        },
    ]
}

fn build_status_hook_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let hooks = profile.status_hooks.as_ref();

    let (enabled, o1) = resolve_value(
        scope,
        global.status_hooks.enabled,
        hooks.and_then(|h| h.enabled),
    );
    let (debounce_ms, debounce_override) = resolve_value(
        scope,
        global.status_hooks.debounce_ms,
        hooks.and_then(|h| h.debounce_ms),
    );
    let (on_starting, o2) = resolve_optional(
        scope,
        global.status_hooks.on_starting.clone(),
        hooks.and_then(|h| h.on_starting.clone()),
        hooks.map(|h| h.on_starting.is_some()).unwrap_or(false),
    );
    let (on_running, o3) = resolve_optional(
        scope,
        global.status_hooks.on_running.clone(),
        hooks.and_then(|h| h.on_running.clone()),
        hooks.map(|h| h.on_running.is_some()).unwrap_or(false),
    );
    let (on_waiting, o4) = resolve_optional(
        scope,
        global.status_hooks.on_waiting.clone(),
        hooks.and_then(|h| h.on_waiting.clone()),
        hooks.map(|h| h.on_waiting.is_some()).unwrap_or(false),
    );
    let (on_idle, o5) = resolve_optional(
        scope,
        global.status_hooks.on_idle.clone(),
        hooks.and_then(|h| h.on_idle.clone()),
        hooks.map(|h| h.on_idle.is_some()).unwrap_or(false),
    );
    let (on_error, o6) = resolve_optional(
        scope,
        global.status_hooks.on_error.clone(),
        hooks.and_then(|h| h.on_error.clone()),
        hooks.map(|h| h.on_error.is_some()).unwrap_or(false),
    );
    let (on_change, o7) = resolve_optional(
        scope,
        global.status_hooks.on_change.clone(),
        hooks.and_then(|h| h.on_change.clone()),
        hooks.map(|h| h.on_change.is_some()).unwrap_or(false),
    );

    vec![
        SettingField {
            key: FieldKey::StatusHooksEnabled,
            label: "Enabled",
            description: "Run local commands when TUI sessions change status",
            value: FieldValue::Bool(enabled),
            category: SettingsCategory::StatusHooks,
            has_override: o1,
            inherited_display: inherited_if(o1, FieldValue::Bool(global.status_hooks.enabled)),
        },
        SettingField {
            key: FieldKey::StatusHookDebounceMs,
            label: "Debounce (ms)",
            description: "Milliseconds a status must remain stable before running hook commands",
            value: FieldValue::Number(debounce_ms),
            category: SettingsCategory::StatusHooks,
            has_override: debounce_override,
            inherited_display: inherited_if(
                debounce_override,
                FieldValue::Number(global.status_hooks.debounce_ms),
            ),
        },
        SettingField {
            key: FieldKey::StatusHookOnStarting,
            label: "On Starting",
            description: "Shell command run when a session enters Starting",
            value: FieldValue::OptionalText(on_starting),
            category: SettingsCategory::StatusHooks,
            has_override: o2,
            inherited_display: inherited_if(
                o2,
                FieldValue::OptionalText(global.status_hooks.on_starting.clone()),
            ),
        },
        SettingField {
            key: FieldKey::StatusHookOnRunning,
            label: "On Running",
            description: "Shell command run when a session enters Running",
            value: FieldValue::OptionalText(on_running),
            category: SettingsCategory::StatusHooks,
            has_override: o3,
            inherited_display: inherited_if(
                o3,
                FieldValue::OptionalText(global.status_hooks.on_running.clone()),
            ),
        },
        SettingField {
            key: FieldKey::StatusHookOnWaiting,
            label: "On Waiting",
            description: "Shell command run when a session enters Waiting",
            value: FieldValue::OptionalText(on_waiting),
            category: SettingsCategory::StatusHooks,
            has_override: o4,
            inherited_display: inherited_if(
                o4,
                FieldValue::OptionalText(global.status_hooks.on_waiting.clone()),
            ),
        },
        SettingField {
            key: FieldKey::StatusHookOnIdle,
            label: "On Idle",
            description: "Shell command run when a session enters Idle",
            value: FieldValue::OptionalText(on_idle),
            category: SettingsCategory::StatusHooks,
            has_override: o5,
            inherited_display: inherited_if(
                o5,
                FieldValue::OptionalText(global.status_hooks.on_idle.clone()),
            ),
        },
        SettingField {
            key: FieldKey::StatusHookOnError,
            label: "On Error",
            description: "Shell command run when a session enters Error",
            value: FieldValue::OptionalText(on_error),
            category: SettingsCategory::StatusHooks,
            has_override: o6,
            inherited_display: inherited_if(
                o6,
                FieldValue::OptionalText(global.status_hooks.on_error.clone()),
            ),
        },
        SettingField {
            key: FieldKey::StatusHookOnChange,
            label: "On Any Change",
            description:
                "Shell command run after the status-specific command on every status change",
            value: FieldValue::OptionalText(on_change),
            category: SettingsCategory::StatusHooks,
            has_override: o7,
            inherited_display: inherited_if(
                o7,
                FieldValue::OptionalText(global.status_hooks.on_change.clone()),
            ),
        },
    ]
}

/// Apply a field's value back to the appropriate config.
/// For profile scope, the value is always stored as an override.
pub fn apply_field_to_config(
    field: &SettingField,
    scope: SettingsScope,
    global: &mut Config,
    profile: &mut ProfileConfig,
) {
    match scope {
        SettingsScope::Global => apply_field_to_global(field, global),
        SettingsScope::Profile | SettingsScope::Repo => {
            apply_field_to_profile(field, global, profile)
        }
    }
}

fn apply_field_to_global(field: &SettingField, config: &mut Config) {
    match (&field.key, &field.value) {
        // ProfileDescription is profile-only; the field never appears in
        // Global scope, but match it so the fallthrough doesn't have to.
        (FieldKey::ProfileDescription, _) => {}
        // Theme
        (FieldKey::ThemeName, FieldValue::Select { selected, options }) => {
            config.theme.name = options.get(*selected).cloned().unwrap_or_default();
        }
        (FieldKey::ThemeColorMode, FieldValue::Select { selected, .. }) => {
            config.theme.color_mode = match selected {
                1 => crate::session::config::ColorMode::Palette,
                _ => crate::session::config::ColorMode::Truecolor,
            };
        }
        (FieldKey::IdleDecayMinutes, FieldValue::Number(v)) => {
            config.theme.idle_decay_minutes = *v;
        }
        // Updates
        (FieldKey::CheckEnabled, FieldValue::Bool(v)) => config.updates.check_enabled = *v,
        (FieldKey::CheckIntervalHours, FieldValue::Number(v)) => {
            config.updates.check_interval_hours = *v
        }
        (FieldKey::NotifyInCli, FieldValue::Bool(v)) => config.updates.notify_in_cli = *v,
        (FieldKey::WebPollIntervalMinutes, FieldValue::Number(v)) => {
            config.updates.web_poll_interval_minutes = *v
        }
        // Worktree
        (FieldKey::WorktreeEnabled, FieldValue::Bool(v)) => config.worktree.enabled = *v,
        (FieldKey::PathTemplate, FieldValue::Text(v)) => config.worktree.path_template = v.clone(),
        (FieldKey::BareRepoPathTemplate, FieldValue::Text(v)) => {
            config.worktree.bare_repo_path_template = v.clone()
        }
        (FieldKey::WorktreeAutoCleanup, FieldValue::Bool(v)) => config.worktree.auto_cleanup = *v,
        (FieldKey::DeleteBranchOnCleanup, FieldValue::Bool(v)) => {
            config.worktree.delete_branch_on_cleanup = *v
        }
        (FieldKey::WorkspacePathTemplate, FieldValue::Text(v)) => {
            config.worktree.workspace_path_template = v.clone()
        }
        (FieldKey::InitSubmodules, FieldValue::Bool(v)) => config.worktree.init_submodules = *v,
        // Sandbox
        (FieldKey::SandboxEnabledByDefault, FieldValue::Bool(v)) => {
            config.sandbox.enabled_by_default = *v
        }
        (FieldKey::YoloModeDefault, FieldValue::Bool(v)) => config.session.yolo_mode_default = *v,
        (FieldKey::StrictHotkeys, FieldValue::Bool(v)) => config.session.strict_hotkeys = *v,
        (FieldKey::AgentStatusHooks, FieldValue::Bool(v)) => {
            config.session.agent_status_hooks = *v;
        }
        (FieldKey::DefaultImage, FieldValue::Text(v)) => config.sandbox.default_image = v.clone(),
        (FieldKey::Environment, FieldValue::List(v)) => config.sandbox.environment = v.clone(),
        (FieldKey::ExtraVolumes, FieldValue::List(v)) => config.sandbox.extra_volumes = v.clone(),
        (FieldKey::PortMappings, FieldValue::List(v)) => config.sandbox.port_mappings = v.clone(),
        (FieldKey::VolumeIgnores, FieldValue::List(v)) => config.sandbox.volume_ignores = v.clone(),
        (FieldKey::MountSsh, FieldValue::Bool(v)) => config.sandbox.mount_ssh = *v,
        (FieldKey::SandboxAutoCleanup, FieldValue::Bool(v)) => config.sandbox.auto_cleanup = *v,
        (FieldKey::CpuLimit, FieldValue::OptionalText(v)) => {
            config.sandbox.cpu_limit = v.clone();
        }
        (FieldKey::MemoryLimit, FieldValue::OptionalText(v)) => {
            config.sandbox.memory_limit = v.clone();
        }
        (FieldKey::CustomInstruction, FieldValue::OptionalText(v)) => {
            config.sandbox.custom_instruction = v.clone();
        }
        (FieldKey::DefaultTerminalMode, FieldValue::Select { selected, .. }) => {
            config.sandbox.default_terminal_mode = match selected {
                0 => DefaultTerminalMode::Host,
                _ => DefaultTerminalMode::Container,
            };
        }
        (FieldKey::ContainerRuntime, FieldValue::Select { selected, .. }) => {
            config.sandbox.container_runtime = match selected {
                0 => ContainerRuntimeName::Docker,
                1 => ContainerRuntimeName::Podman,
                _ => ContainerRuntimeName::AppleContainer,
            };
        }
        // Tmux
        (FieldKey::StatusBar, FieldValue::Select { selected, .. }) => {
            config.tmux.status_bar = match selected {
                0 => TmuxStatusBarMode::Auto,
                1 => TmuxStatusBarMode::Enabled,
                _ => TmuxStatusBarMode::Disabled,
            };
        }
        (FieldKey::Mouse, FieldValue::Select { selected, .. }) => {
            config.tmux.mouse = match selected {
                0 => TmuxMouseMode::Auto,
                1 => TmuxMouseMode::Enabled,
                _ => TmuxMouseMode::Disabled,
            };
        }
        (FieldKey::Clipboard, FieldValue::Select { selected, .. }) => {
            config.tmux.clipboard = match selected {
                0 => TmuxClipboardMode::Auto,
                1 => TmuxClipboardMode::Enabled,
                _ => TmuxClipboardMode::Disabled,
            };
        }
        // Session
        (FieldKey::DefaultTool, FieldValue::Select { selected, .. }) => {
            config.session.default_tool =
                crate::agents::name_from_settings_index(*selected).map(|s| s.to_string());
        }
        (FieldKey::AgentExtraArgs, FieldValue::List(v)) => {
            config.session.agent_extra_args = parse_key_value_list(v);
        }
        (FieldKey::AgentCommandOverride, FieldValue::List(v)) => {
            config.session.agent_command_override = parse_key_value_list(v);
        }
        (FieldKey::CustomAgents, FieldValue::List(v)) => {
            config.session.custom_agents = parse_key_value_list(v);
        }
        (FieldKey::AgentDetectAs, FieldValue::List(v)) => {
            config.session.agent_detect_as = parse_key_value_list(v);
        }
        // Sound
        (FieldKey::SoundEnabled, FieldValue::Bool(v)) => config.sound.enabled = *v,
        (FieldKey::SoundMode, FieldValue::Select { selected, .. }) => {
            config.sound.mode = match selected {
                1 => SoundMode::Specific(String::new()),
                _ => SoundMode::Random,
            };
        }
        (FieldKey::SoundVolume, FieldValue::Select { selected, options }) => {
            if let Some(s) = options.get(*selected) {
                config.sound.volume = volume_from_option(s);
            }
        }
        (FieldKey::SoundOnStart, FieldValue::OptionalText(v)) => {
            config.sound.on_start = v.clone();
        }
        (FieldKey::SoundOnRunning, FieldValue::OptionalText(v)) => {
            config.sound.on_running = v.clone();
        }
        (FieldKey::SoundOnWaiting, FieldValue::OptionalText(v)) => {
            config.sound.on_waiting = v.clone();
        }
        (FieldKey::SoundOnIdle, FieldValue::OptionalText(v)) => {
            config.sound.on_idle = v.clone();
        }
        (FieldKey::SoundOnError, FieldValue::OptionalText(v)) => {
            config.sound.on_error = v.clone();
        }
        (FieldKey::SoundOnApproval, FieldValue::OptionalText(v)) => {
            config.sound.on_approval = v.clone();
        }
        // Status hooks
        (FieldKey::StatusHooksEnabled, FieldValue::Bool(v)) => {
            config.status_hooks.enabled = *v;
        }
        (FieldKey::StatusHookDebounceMs, FieldValue::Number(v)) => {
            config.status_hooks.debounce_ms = *v;
        }
        (FieldKey::StatusHookOnStarting, FieldValue::OptionalText(v)) => {
            config.status_hooks.on_starting = v.clone();
        }
        (FieldKey::StatusHookOnRunning, FieldValue::OptionalText(v)) => {
            config.status_hooks.on_running = v.clone();
        }
        (FieldKey::StatusHookOnWaiting, FieldValue::OptionalText(v)) => {
            config.status_hooks.on_waiting = v.clone();
        }
        (FieldKey::StatusHookOnIdle, FieldValue::OptionalText(v)) => {
            config.status_hooks.on_idle = v.clone();
        }
        (FieldKey::StatusHookOnError, FieldValue::OptionalText(v)) => {
            config.status_hooks.on_error = v.clone();
        }
        (FieldKey::StatusHookOnChange, FieldValue::OptionalText(v)) => {
            config.status_hooks.on_change = v.clone();
        }
        // Hooks
        (FieldKey::HookOnCreate, FieldValue::List(v)) => config.hooks.on_create = v.clone(),
        (FieldKey::HookOnLaunch, FieldValue::List(v)) => config.hooks.on_launch = v.clone(),
        (FieldKey::HookOnDestroy, FieldValue::List(v)) => config.hooks.on_destroy = v.clone(),
        // Web
        (FieldKey::WebNotificationsEnabled, FieldValue::Bool(v)) => {
            config.web.notifications_enabled = *v;
        }
        (FieldKey::WebNotifyOnWaiting, FieldValue::Bool(v)) => {
            config.web.notify_on_waiting = *v;
        }
        (FieldKey::WebNotifyOnIdle, FieldValue::Bool(v)) => {
            config.web.notify_on_idle = *v;
        }
        (FieldKey::WebNotifyOnWakeFire, FieldValue::Bool(v)) => {
            config.web.notify_on_wake_fire = *v;
        }
        (FieldKey::WebNotifyOnError, FieldValue::Bool(v)) => {
            config.web.notify_on_error = *v;
        }
        // Cockpit
        (FieldKey::CockpitEnabled, FieldValue::Bool(v)) => config.cockpit.enabled = *v,
        (FieldKey::CockpitDefaultForClaude, FieldValue::Bool(v)) => {
            config.cockpit.default_for_claude = *v
        }
        (FieldKey::CockpitDefaultAgent, FieldValue::Text(v)) => {
            config.cockpit.default_agent = v.clone()
        }
        (FieldKey::CockpitMaxConcurrentWorkers, FieldValue::Number(v)) => {
            config.cockpit.max_concurrent_workers = (*v).max(1) as u32
        }
        (FieldKey::CockpitReplayEvents, FieldValue::Number(v)) => {
            // 0 = unlimited history; clamp non-zero values to u32 range.
            // See #1065.
            config.cockpit.replay_events = (*v).min(u32::MAX as u64) as u32
        }
        (FieldKey::CockpitReplayBytes, FieldValue::Number(v)) => {
            config.cockpit.replay_bytes = (*v).max(1024)
        }
        (FieldKey::CockpitNodePath, FieldValue::Text(v)) => config.cockpit.node_path = v.clone(),
        (FieldKey::CockpitShowToolDurations, FieldValue::Bool(v)) => {
            config.cockpit.show_tool_durations = *v
        }
        (FieldKey::CockpitQueueDrainMode, FieldValue::Select { selected, options }) => {
            if let Some(name) = options.get(*selected) {
                if let Some(mode) = crate::session::config::QueueDrainMode::parse(name) {
                    config.cockpit.queue_drain_mode = mode;
                }
            }
        }
        (FieldKey::CockpitMaxConcurrentResumes, FieldValue::Number(v)) => {
            config.cockpit.max_concurrent_resumes = (*v).max(1).min(u32::MAX as u64) as u32
        }
        (FieldKey::CockpitForceEndTurnThresholdSecs, FieldValue::Number(v)) => {
            config.cockpit.force_end_turn_threshold_secs = (*v).max(1).min(u32::MAX as u64) as u32
        }
        // Logging
        (FieldKey::LoggingDefaultLevel, FieldValue::Select { selected, options }) => {
            if let Some(level) = options.get(*selected) {
                config.logging.default_level = level.clone();
            }
        }
        (FieldKey::LoggingTarget(idx), FieldValue::Select { selected, options }) => {
            if let Some(target) = crate::logging::KNOWN_SUB_TARGETS.get(*idx as usize) {
                let level = options.get(*selected).cloned().unwrap_or_default();
                if level.is_empty() || level == "(default)" {
                    config.logging.targets.remove(*target);
                } else {
                    config.logging.targets.insert(target.to_string(), level);
                }
            }
        }
        (FieldKey::LoggingOutput, FieldValue::Select { selected, options }) => {
            if let Some(value) = options.get(*selected) {
                config.logging.output = match value.as_str() {
                    "stdout" => crate::session::config::SinkKind::Stdout,
                    _ => crate::session::config::SinkKind::File,
                };
            }
        }
        (FieldKey::LoggingFilePath, FieldValue::Text(v)) => {
            let trimmed = v.trim();
            config.logging.file_path = if trimmed.is_empty() {
                "debug.log".to_string()
            } else {
                trimmed.to_string()
            };
        }
        (FieldKey::LoggingRotation, FieldValue::Select { selected, options }) => {
            if let Some(value) = options.get(*selected) {
                config.logging.rotation = match value.as_str() {
                    "never" => crate::session::config::RotationKind::Never,
                    _ => crate::session::config::RotationKind::Size,
                };
            }
        }
        (FieldKey::LoggingMaxSizeMib, FieldValue::Number(v)) => {
            config.logging.max_size_mib = (*v).max(1);
        }
        (FieldKey::LoggingKeepCount, FieldValue::Number(v)) => {
            config.logging.keep_count = (*v).clamp(1, u8::MAX as u64) as u8;
        }
        (FieldKey::LoggingShowSpans, FieldValue::Bool(v)) => {
            config.logging.show_spans = *v;
        }
        (FieldKey::HostEnvironment, FieldValue::List(v)) => config.environment = v.clone(),
        _ => {}
    }
}

/// Apply a field to the profile config.
/// Always stores the value as an override; use 'r' key to clear overrides.
fn apply_field_to_profile(field: &SettingField, _global: &Config, config: &mut ProfileConfig) {
    match (&field.key, &field.value) {
        // Profile description: empty input clears the field, otherwise we
        // store the trimmed string so a stray space doesn't promote the
        // profile to "has overrides".
        (FieldKey::ProfileDescription, FieldValue::OptionalText(v)) => {
            config.description = v
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
        // Theme
        (FieldKey::ThemeName, FieldValue::Select { selected, options }) => {
            let name = options.get(*selected).cloned().unwrap_or_default();
            use crate::session::ThemeConfigOverride;
            let t = config
                .theme
                .get_or_insert_with(ThemeConfigOverride::default);
            t.name = Some(name);
        }
        (FieldKey::ThemeColorMode, FieldValue::Select { selected, .. }) => {
            use crate::session::ThemeConfigOverride;
            let t = config
                .theme
                .get_or_insert_with(ThemeConfigOverride::default);
            t.color_mode = Some(match selected {
                1 => crate::session::config::ColorMode::Palette,
                _ => crate::session::config::ColorMode::Truecolor,
            });
        }
        (FieldKey::IdleDecayMinutes, FieldValue::Number(v)) => {
            use crate::session::ThemeConfigOverride;
            let t = config
                .theme
                .get_or_insert_with(ThemeConfigOverride::default);
            t.idle_decay_minutes = Some(*v);
        }
        // Updates
        (FieldKey::CheckEnabled, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.updates, |s, val| s.check_enabled = val);
        }
        (FieldKey::CheckIntervalHours, FieldValue::Number(v)) => {
            set_profile_override(*v, &mut config.updates, |s, val| {
                s.check_interval_hours = val
            });
        }
        (FieldKey::NotifyInCli, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.updates, |s, val| s.notify_in_cli = val);
        }
        (FieldKey::WebPollIntervalMinutes, FieldValue::Number(v)) => {
            set_profile_override(*v, &mut config.updates, |s, val| {
                s.web_poll_interval_minutes = val
            });
        }
        // Worktree
        (FieldKey::WorktreeEnabled, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.worktree, |s, val| s.enabled = val);
        }
        (FieldKey::PathTemplate, FieldValue::Text(v)) => {
            set_profile_override(v.clone(), &mut config.worktree, |s, val| {
                s.path_template = val
            });
        }
        (FieldKey::BareRepoPathTemplate, FieldValue::Text(v)) => {
            set_profile_override(v.clone(), &mut config.worktree, |s, val| {
                s.bare_repo_path_template = val
            });
        }
        (FieldKey::WorktreeAutoCleanup, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.worktree, |s, val| s.auto_cleanup = val);
        }
        (FieldKey::DeleteBranchOnCleanup, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.worktree, |s, val| {
                s.delete_branch_on_cleanup = val
            });
        }
        (FieldKey::WorkspacePathTemplate, FieldValue::Text(v)) => {
            set_profile_override(v.clone(), &mut config.worktree, |s, val| {
                s.workspace_path_template = val
            });
        }
        (FieldKey::InitSubmodules, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.worktree, |s, val| s.init_submodules = val);
        }
        // Sandbox
        (FieldKey::SandboxEnabledByDefault, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.sandbox, |s, val| s.enabled_by_default = val);
        }
        (FieldKey::DefaultImage, FieldValue::Text(v)) => {
            set_profile_override(v.clone(), &mut config.sandbox, |s, val| {
                s.default_image = val
            });
        }
        (FieldKey::Environment, FieldValue::List(v)) => {
            set_profile_override(v.clone(), &mut config.sandbox, |s, val| s.environment = val);
        }
        (FieldKey::ExtraVolumes, FieldValue::List(v)) => {
            set_profile_override(v.clone(), &mut config.sandbox, |s, val| {
                s.extra_volumes = val
            });
        }
        (FieldKey::PortMappings, FieldValue::List(v)) => {
            set_profile_override(v.clone(), &mut config.sandbox, |s, val| {
                s.port_mappings = val
            });
        }
        (FieldKey::VolumeIgnores, FieldValue::List(v)) => {
            set_profile_override(v.clone(), &mut config.sandbox, |s, val| {
                s.volume_ignores = val
            });
        }
        (FieldKey::MountSsh, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.sandbox, |s, val| s.mount_ssh = val);
        }
        (FieldKey::SandboxAutoCleanup, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.sandbox, |s, val| s.auto_cleanup = val);
        }
        (FieldKey::CpuLimit, FieldValue::OptionalText(v)) => {
            use crate::session::SandboxConfigOverride;
            let s = config
                .sandbox
                .get_or_insert_with(SandboxConfigOverride::default);
            s.cpu_limit = v.clone();
        }
        (FieldKey::MemoryLimit, FieldValue::OptionalText(v)) => {
            use crate::session::SandboxConfigOverride;
            let s = config
                .sandbox
                .get_or_insert_with(SandboxConfigOverride::default);
            s.memory_limit = v.clone();
        }
        (FieldKey::CustomInstruction, FieldValue::OptionalText(v)) => {
            use crate::session::SandboxConfigOverride;
            let s = config
                .sandbox
                .get_or_insert_with(SandboxConfigOverride::default);
            s.custom_instruction = v.clone();
        }
        (FieldKey::DefaultTerminalMode, FieldValue::Select { selected, .. }) => {
            let mode = match selected {
                0 => DefaultTerminalMode::Host,
                _ => DefaultTerminalMode::Container,
            };
            set_profile_override(mode, &mut config.sandbox, |s, val| {
                s.default_terminal_mode = val
            });
        }
        (FieldKey::ContainerRuntime, FieldValue::Select { selected, .. }) => {
            let runtime = match selected {
                0 => ContainerRuntimeName::Docker,
                1 => ContainerRuntimeName::Podman,
                _ => ContainerRuntimeName::AppleContainer,
            };
            set_profile_override(runtime, &mut config.sandbox, |s, val| {
                s.container_runtime = val
            });
        }
        // Tmux
        (FieldKey::StatusBar, FieldValue::Select { selected, .. }) => {
            let mode = match selected {
                0 => TmuxStatusBarMode::Auto,
                1 => TmuxStatusBarMode::Enabled,
                _ => TmuxStatusBarMode::Disabled,
            };
            set_profile_override(mode, &mut config.tmux, |s, val| s.status_bar = val);
        }
        (FieldKey::Mouse, FieldValue::Select { selected, .. }) => {
            let mode = match selected {
                0 => TmuxMouseMode::Auto,
                1 => TmuxMouseMode::Enabled,
                _ => TmuxMouseMode::Disabled,
            };
            set_profile_override(mode, &mut config.tmux, |s, val| s.mouse = val);
        }
        (FieldKey::Clipboard, FieldValue::Select { selected, .. }) => {
            let mode = match selected {
                0 => TmuxClipboardMode::Auto,
                1 => TmuxClipboardMode::Enabled,
                _ => TmuxClipboardMode::Disabled,
            };
            set_profile_override(mode, &mut config.tmux, |s, val| s.clipboard = val);
        }
        // Session
        (FieldKey::DefaultTool, FieldValue::Select { selected, .. }) => {
            let tool = crate::agents::name_from_settings_index(*selected).map(|s| s.to_string());
            use crate::session::SessionConfigOverride;
            let session = config
                .session
                .get_or_insert_with(SessionConfigOverride::default);
            session.default_tool = tool;
        }
        (FieldKey::YoloModeDefault, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.session, |s, val| s.yolo_mode_default = val);
        }
        (FieldKey::StrictHotkeys, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.session, |s, val| s.strict_hotkeys = val);
        }
        (FieldKey::AgentStatusHooks, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.session, |s, val| {
                s.agent_status_hooks = val;
            });
        }
        (FieldKey::AgentExtraArgs, FieldValue::List(v)) => {
            let map = parse_key_value_list(v);
            use crate::session::SessionConfigOverride;
            let s = config
                .session
                .get_or_insert_with(SessionConfigOverride::default);
            s.agent_extra_args = Some(map);
        }
        (FieldKey::AgentCommandOverride, FieldValue::List(v)) => {
            let map = parse_key_value_list(v);
            use crate::session::SessionConfigOverride;
            let s = config
                .session
                .get_or_insert_with(SessionConfigOverride::default);
            s.agent_command_override = Some(map);
        }
        (FieldKey::CustomAgents, FieldValue::List(v)) => {
            let map = parse_key_value_list(v);
            use crate::session::SessionConfigOverride;
            let s = config
                .session
                .get_or_insert_with(SessionConfigOverride::default);
            s.custom_agents = Some(map);
        }
        (FieldKey::AgentDetectAs, FieldValue::List(v)) => {
            let map = parse_key_value_list(v);
            use crate::session::SessionConfigOverride;
            let s = config
                .session
                .get_or_insert_with(SessionConfigOverride::default);
            s.agent_detect_as = Some(map);
        }
        // Sound
        (FieldKey::SoundEnabled, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.sound, |s, val| s.enabled = val);
        }
        (FieldKey::SoundMode, FieldValue::Select { selected, .. }) => {
            let mode = match selected {
                1 => SoundMode::Specific(String::new()),
                _ => SoundMode::Random,
            };
            set_profile_override(mode, &mut config.sound, |s, val| s.mode = val);
        }
        (FieldKey::SoundVolume, FieldValue::Select { selected, options }) => {
            if let Some(s) = options.get(*selected) {
                let vol = volume_from_option(s);
                set_profile_override(vol, &mut config.sound, |s, val| s.volume = val);
            }
        }
        (FieldKey::SoundOnStart, FieldValue::OptionalText(v)) => {
            let s = config
                .sound
                .get_or_insert_with(crate::sound::SoundConfigOverride::default);
            s.on_start = v.clone();
        }
        (FieldKey::SoundOnRunning, FieldValue::OptionalText(v)) => {
            let s = config
                .sound
                .get_or_insert_with(crate::sound::SoundConfigOverride::default);
            s.on_running = v.clone();
        }
        (FieldKey::SoundOnWaiting, FieldValue::OptionalText(v)) => {
            let s = config
                .sound
                .get_or_insert_with(crate::sound::SoundConfigOverride::default);
            s.on_waiting = v.clone();
        }
        (FieldKey::SoundOnIdle, FieldValue::OptionalText(v)) => {
            let s = config
                .sound
                .get_or_insert_with(crate::sound::SoundConfigOverride::default);
            s.on_idle = v.clone();
        }
        (FieldKey::SoundOnError, FieldValue::OptionalText(v)) => {
            let s = config
                .sound
                .get_or_insert_with(crate::sound::SoundConfigOverride::default);
            s.on_error = v.clone();
        }
        (FieldKey::SoundOnApproval, FieldValue::OptionalText(v)) => {
            let s = config
                .sound
                .get_or_insert_with(crate::sound::SoundConfigOverride::default);
            s.on_approval = v.clone();
        }
        // Status hooks
        (FieldKey::StatusHooksEnabled, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.status_hooks, |s, val| s.enabled = val);
        }
        (FieldKey::StatusHookDebounceMs, FieldValue::Number(v)) => {
            set_profile_override(*v, &mut config.status_hooks, |s, val| s.debounce_ms = val);
        }
        (FieldKey::StatusHookOnStarting, FieldValue::OptionalText(v)) => {
            let s = config
                .status_hooks
                .get_or_insert_with(crate::status_hooks::StatusHookConfigOverride::default);
            s.on_starting = v.clone();
        }
        (FieldKey::StatusHookOnRunning, FieldValue::OptionalText(v)) => {
            let s = config
                .status_hooks
                .get_or_insert_with(crate::status_hooks::StatusHookConfigOverride::default);
            s.on_running = v.clone();
        }
        (FieldKey::StatusHookOnWaiting, FieldValue::OptionalText(v)) => {
            let s = config
                .status_hooks
                .get_or_insert_with(crate::status_hooks::StatusHookConfigOverride::default);
            s.on_waiting = v.clone();
        }
        (FieldKey::StatusHookOnIdle, FieldValue::OptionalText(v)) => {
            let s = config
                .status_hooks
                .get_or_insert_with(crate::status_hooks::StatusHookConfigOverride::default);
            s.on_idle = v.clone();
        }
        (FieldKey::StatusHookOnError, FieldValue::OptionalText(v)) => {
            let s = config
                .status_hooks
                .get_or_insert_with(crate::status_hooks::StatusHookConfigOverride::default);
            s.on_error = v.clone();
        }
        (FieldKey::StatusHookOnChange, FieldValue::OptionalText(v)) => {
            let s = config
                .status_hooks
                .get_or_insert_with(crate::status_hooks::StatusHookConfigOverride::default);
            s.on_change = v.clone();
        }
        // Hooks
        (FieldKey::HookOnCreate, FieldValue::List(v)) => {
            set_profile_override(v.clone(), &mut config.hooks, |s, val| s.on_create = val);
        }
        (FieldKey::HookOnLaunch, FieldValue::List(v)) => {
            set_profile_override(v.clone(), &mut config.hooks, |s, val| s.on_launch = val);
        }
        (FieldKey::HookOnDestroy, FieldValue::List(v)) => {
            set_profile_override(v.clone(), &mut config.hooks, |s, val| s.on_destroy = val);
        }
        // Cockpit
        (FieldKey::CockpitEnabled, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.cockpit, |s, val| s.enabled = val);
        }
        (FieldKey::CockpitDefaultForClaude, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.cockpit, |s, val| s.default_for_claude = val);
        }
        (FieldKey::CockpitDefaultAgent, FieldValue::Text(v)) => {
            set_profile_override(v.clone(), &mut config.cockpit, |s, val| {
                s.default_agent = val
            });
        }
        (FieldKey::CockpitMaxConcurrentWorkers, FieldValue::Number(v)) => {
            set_profile_override((*v).max(1) as u32, &mut config.cockpit, |s, val| {
                s.max_concurrent_workers = val
            });
        }
        (FieldKey::CockpitReplayEvents, FieldValue::Number(v)) => {
            // 0 = unlimited; #1065.
            set_profile_override(
                (*v).min(u32::MAX as u64) as u32,
                &mut config.cockpit,
                |s, val| s.replay_events = val,
            );
        }
        (FieldKey::CockpitReplayBytes, FieldValue::Number(v)) => {
            set_profile_override((*v).max(1024), &mut config.cockpit, |s, val| {
                s.replay_bytes = val
            });
        }
        (FieldKey::CockpitNodePath, FieldValue::Text(v)) => {
            set_profile_override(v.clone(), &mut config.cockpit, |s, val| s.node_path = val);
        }
        (FieldKey::CockpitShowToolDurations, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.cockpit, |s, val| {
                s.show_tool_durations = val
            });
        }
        (FieldKey::CockpitQueueDrainMode, FieldValue::Select { selected, options }) => {
            if let Some(name) = options.get(*selected) {
                if let Some(mode) = crate::session::config::QueueDrainMode::parse(name) {
                    set_profile_override(mode, &mut config.cockpit, |s, val| {
                        s.queue_drain_mode = val
                    });
                }
            }
        }
        (FieldKey::CockpitMaxConcurrentResumes, FieldValue::Number(v)) => {
            let clamped = (*v).max(1).min(u32::MAX as u64) as u32;
            set_profile_override(clamped, &mut config.cockpit, |s, val| {
                s.max_concurrent_resumes = val
            });
        }
        (FieldKey::CockpitForceEndTurnThresholdSecs, FieldValue::Number(v)) => {
            let clamped = (*v).max(1).min(u32::MAX as u64) as u32;
            set_profile_override(clamped, &mut config.cockpit, |s, val| {
                s.force_end_turn_threshold_secs = val
            });
        }
        (FieldKey::HostEnvironment, FieldValue::List(v)) => {
            // Empty list clears the override (no env entries); otherwise store
            // the list as the profile-scope replacement of the global list.
            config.environment = if v.is_empty() { None } else { Some(v.clone()) };
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Config, ProfileConfig};

    #[test]
    fn test_profile_field_has_no_override_after_global_change() {
        // Start with default configs
        let mut global = Config::default();
        let profile = ProfileConfig::default();

        // Verify initial state - profile shows no override
        let fields = build_fields_for_category(
            SettingsCategory::Updates,
            SettingsScope::Profile,
            &global,
            &profile,
        );

        let check_enabled_field = fields
            .iter()
            .find(|f| f.key == FieldKey::CheckEnabled)
            .unwrap();
        assert!(
            !check_enabled_field.has_override,
            "Profile should not show override initially"
        );

        // Change global setting
        global.updates.check_enabled = !global.updates.check_enabled;

        // Rebuild profile fields - should still show no override
        let fields = build_fields_for_category(
            SettingsCategory::Updates,
            SettingsScope::Profile,
            &global,
            &profile,
        );

        let check_enabled_field = fields
            .iter()
            .find(|f| f.key == FieldKey::CheckEnabled)
            .unwrap();
        assert!(
            !check_enabled_field.has_override,
            "Profile should NOT show override after global change - it should inherit"
        );
    }

    #[test]
    fn test_profile_field_shows_override_after_profile_change() {
        let global = Config::default();
        let mut profile = ProfileConfig::default();

        // Initially no override
        let fields = build_fields_for_category(
            SettingsCategory::Updates,
            SettingsScope::Profile,
            &global,
            &profile,
        );
        let check_enabled_field = fields
            .iter()
            .find(|f| f.key == FieldKey::CheckEnabled)
            .unwrap();
        assert!(!check_enabled_field.has_override);

        // Set a profile override
        profile.updates = Some(crate::session::UpdatesConfigOverride {
            check_enabled: Some(false),
            ..Default::default()
        });

        // Rebuild - should now show override
        let fields = build_fields_for_category(
            SettingsCategory::Updates,
            SettingsScope::Profile,
            &global,
            &profile,
        );
        let check_enabled_field = fields
            .iter()
            .find(|f| f.key == FieldKey::CheckEnabled)
            .unwrap();
        assert!(
            check_enabled_field.has_override,
            "Profile SHOULD show override after explicit profile change"
        );
    }

    #[test]
    fn test_default_tool_options_include_all_registered_agents() {
        let global = Config::default();
        let profile = ProfileConfig::default();

        let fields = build_fields_for_category(
            SettingsCategory::Session,
            SettingsScope::Global,
            &global,
            &profile,
        );

        let tool_field = fields
            .iter()
            .find(|f| f.key == FieldKey::DefaultTool)
            .expect("DefaultTool field should exist");

        let options = match &tool_field.value {
            FieldValue::Select { options, .. } => options,
            _ => panic!("DefaultTool should be a Select field"),
        };

        let tool_options: Vec<&str> = options.iter().skip(1).map(|s| s.as_str()).collect();
        let agent_names = crate::agents::agent_names();

        for name in &agent_names {
            assert!(
                tool_options.contains(name),
                "Settings UI missing agent '{}'. UI options: {:?}",
                name,
                tool_options
            );
        }

        for option in &tool_options {
            assert!(
                agent_names.contains(option),
                "Settings UI has unknown agent '{}' not in registry.",
                option
            );
        }
    }

    #[test]
    fn test_profile_override_preserved_when_matching_global() {
        let global = Config::default();
        let mut profile = ProfileConfig::default();

        // Set a profile override that matches the global value
        let global_check_enabled = global.updates.check_enabled;
        profile.updates = Some(crate::session::UpdatesConfigOverride {
            check_enabled: Some(global_check_enabled),
            ..Default::default()
        });

        // Apply the same value through the field system
        let fields = build_fields_for_category(
            SettingsCategory::Updates,
            SettingsScope::Profile,
            &global,
            &profile,
        );
        let field = fields
            .iter()
            .find(|f| f.key == FieldKey::CheckEnabled)
            .unwrap();

        // Re-apply the field (simulates user saving without changing the value)
        apply_field_to_profile(field, &global, &mut profile);

        // The override should still be present
        assert!(
            profile
                .updates
                .as_ref()
                .and_then(|u| u.check_enabled)
                .is_some(),
            "Profile override should be preserved even when value matches global"
        );
    }

    #[test]
    fn test_bool_toggle_back_to_global_preserves_override() {
        let global = Config::default();
        let mut profile = ProfileConfig::default();
        let original = global.updates.check_enabled;

        // Toggle to non-global value
        profile.updates = Some(crate::session::UpdatesConfigOverride {
            check_enabled: Some(!original),
            ..Default::default()
        });

        // Now toggle back to match global
        let field = SettingField {
            key: FieldKey::CheckEnabled,
            label: "Check Enabled",
            description: "",
            value: FieldValue::Bool(original),
            category: SettingsCategory::Updates,
            has_override: true,
            inherited_display: None,
        };

        apply_field_to_profile(&field, &global, &mut profile);

        // Override should still be present (not silently cleared)
        assert!(
            profile
                .updates
                .as_ref()
                .and_then(|u| u.check_enabled)
                .is_some(),
            "Toggling back to match global should preserve the override, not silently clear it"
        );
        assert_eq!(
            profile.updates.as_ref().unwrap().check_enabled,
            Some(original),
            "Override value should match what was set"
        );
    }

    #[test]
    fn test_worktree_enabled_field_uses_existing_config_value() {
        let mut global = Config::default();
        global.worktree.enabled = true;
        let profile = ProfileConfig::default();

        let fields = build_fields_for_category(
            SettingsCategory::Worktree,
            SettingsScope::Global,
            &global,
            &profile,
        );
        let field = fields
            .iter()
            .find(|f| f.key == FieldKey::WorktreeEnabled)
            .unwrap();

        assert_eq!(field.label, "Enabled by Default");
        assert!(matches!(field.value, FieldValue::Bool(true)));
    }

    #[test]
    fn test_worktree_enabled_profile_override() {
        let global = Config::default();
        let profile = ProfileConfig {
            worktree: Some(crate::session::WorktreeConfigOverride {
                enabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };

        let fields = build_fields_for_category(
            SettingsCategory::Worktree,
            SettingsScope::Profile,
            &global,
            &profile,
        );
        let field = fields
            .iter()
            .find(|f| f.key == FieldKey::WorktreeEnabled)
            .unwrap();

        assert!(field.has_override);
        assert!(matches!(field.value, FieldValue::Bool(true)));
    }

    #[test]
    fn test_status_hook_debounce_field_uses_default() {
        let global = Config::default();
        let profile = ProfileConfig::default();

        let fields = build_fields_for_category(
            SettingsCategory::StatusHooks,
            SettingsScope::Global,
            &global,
            &profile,
        );
        let field = fields
            .iter()
            .find(|f| f.key == FieldKey::StatusHookDebounceMs)
            .expect("StatusHookDebounceMs field should exist");

        assert_eq!(field.label, "Debounce (ms)");
        assert!(matches!(field.value, FieldValue::Number(100)));
    }

    #[test]
    fn test_status_hook_debounce_profile_override() {
        let global = Config::default();
        let profile = ProfileConfig {
            status_hooks: Some(crate::status_hooks::StatusHookConfigOverride {
                debounce_ms: Some(500),
                ..Default::default()
            }),
            ..Default::default()
        };

        let fields = build_fields_for_category(
            SettingsCategory::StatusHooks,
            SettingsScope::Profile,
            &global,
            &profile,
        );
        let field = fields
            .iter()
            .find(|f| f.key == FieldKey::StatusHookDebounceMs)
            .expect("StatusHookDebounceMs field should exist");

        assert!(field.has_override);
        assert!(matches!(field.value, FieldValue::Number(500)));
        assert_eq!(field.inherited_display.as_deref(), Some("100"));
    }

    #[test]
    fn test_status_hook_debounce_apply_global_and_profile() {
        let mut global = Config::default();
        let mut profile = ProfileConfig::default();
        let field = SettingField {
            key: FieldKey::StatusHookDebounceMs,
            label: "Debounce (ms)",
            description: "",
            value: FieldValue::Number(250),
            category: SettingsCategory::StatusHooks,
            has_override: false,
            inherited_display: None,
        };

        apply_field_to_config(&field, SettingsScope::Global, &mut global, &mut profile);
        assert_eq!(global.status_hooks.debounce_ms, 250);

        apply_field_to_config(&field, SettingsScope::Profile, &mut global, &mut profile);
        assert_eq!(
            profile.status_hooks.as_ref().and_then(|s| s.debounce_ms),
            Some(250)
        );
    }
}
