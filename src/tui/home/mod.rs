//! Home view - main session list and navigation

mod input;
mod operations;
mod render;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ratatui::prelude::Rect;
use tui_input::Input;

use crate::session::{
    config::{load_config, save_config, GroupByMode, SortOrder},
    flatten_tree, flatten_tree_all_profiles, resolve_config_or_warn, DefaultTerminalMode,
    EnsureReadyOutcome, Group, GroupTree, Instance, Item, Storage,
};
use crate::tmux::AvailableTools;

use super::creation_poller::{CreationPoller, CreationRequest};
use super::deletion_poller::DeletionPoller;
#[cfg(feature = "serve")]
use super::dialogs::ServeView;
use super::dialogs::{
    ChangelogDialog, CommandPaletteDialog, ConfirmDialog, GroupDeleteOptionsDialog,
    HookTrustDialog, HooksInstallDialog, InfoDialog, NewSessionData, NewSessionDialog,
    NoAgentsDialog, ProfilePickerDialog, ProjectsDialog, RenameDialog, UnifiedDeleteDialog,
    UpdateConfirmDialog, WelcomeDialog,
};
use super::diff::DiffView;
use super::settings::SettingsView;
use super::status_poller::{StatusPoller, StatusUpdate};

/// Extract a project group name from a session instance.
/// Uses `worktree_info.main_repo_path` for worktree sessions (so all branches of the
/// same repo group together), otherwise uses `project_path`. Returns the last path segment.
fn project_group_name(inst: &Instance) -> String {
    let base_path = inst
        .worktree_info
        .as_ref()
        .map(|wt| wt.main_repo_path.as_str())
        .unwrap_or(&inst.project_path);

    let path = std::path::Path::new(base_path);
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| {
            // For root paths like "/", use a readable fallback
            if base_path == "/" || base_path.is_empty() {
                "(root)".to_string()
            } else {
                base_path.to_string()
            }
        })
}

pub(super) struct GroupRenameContext {
    pub(super) old_path: String,
    pub(super) old_profile: String,
}

/// View mode for the home screen
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Agent,
    Terminal,
    /// Previewing a tool session (lazygit, yazi, etc.)
    Tool(String),
}

/// Terminal mode for sandboxed sessions (container vs host)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerminalMode {
    #[default]
    Host,
    Container,
}

/// Cached preview content to avoid subprocess calls on every frame
pub(super) struct PreviewCache {
    pub(super) session_id: Option<String>,
    pub(super) content: String,
    pub(super) last_refresh: Instant,
    pub(super) dimensions: (u16, u16),
    /// Number of lines that were captured into `content`. Used together with
    /// the BUFFER reserve so consecutive wheel ticks don't trigger a fresh
    /// `tmux capture-pane` subprocess while the cached window still covers
    /// the requested scroll.
    pub(super) captured_lines: usize,
}

impl Default for PreviewCache {
    fn default() -> Self {
        Self {
            session_id: None,
            content: String::new(),
            last_refresh: Instant::now(),
            dimensions: (0, 0),
            captured_lines: 0,
        }
    }
}

pub(super) const INDENTS: [&str; 10] = [
    "",
    " ",
    "  ",
    "   ",
    "    ",
    "     ",
    "      ",
    "       ",
    "        ",
    "         ",
];

pub(super) fn get_indent(depth: usize) -> &'static str {
    INDENTS.get(depth).copied().unwrap_or(INDENTS[9])
}

pub(super) const ICON_IDLE: &str = "⠒";
pub(super) const ICON_ERROR: &str = "✕";
pub(super) const ICON_UNKNOWN: &str = "⠤";
pub(super) const ICON_STOPPED: &str = "⠒";
pub(super) const ICON_DELETING: &str = "✕";
pub(super) const ICON_COLLAPSED: &str = "▶";
pub(super) const ICON_EXPANDED: &str = "▼";

/// Hook progress for a session being created in the background
pub(super) struct CreatingHookProgress {
    pub(super) hook_output: Vec<String>,
    pub(super) current_hook: Option<String>,
}

/// Result delivered by a startup-recovery worker back to the TUI tick.
struct RecoveryUpdate {
    instance_id: String,
    title: String,
    /// Updated `Instance` snapshot (post-cascade), so the TUI can replace
    /// its in-memory copy without a disk reload that would lose the
    /// freshly-set `last_start_time` (which is `#[serde(skip)]`).
    instance: Box<crate::session::Instance>,
    result: Result<crate::session::StartOutcome, String>,
}

pub struct HomeView {
    pub(super) storages: HashMap<String, Storage>,
    pub(super) active_profile: Option<String>,
    instances: Vec<Instance>,
    instance_map: HashMap<String, Instance>,
    pub(super) group_trees: HashMap<String, GroupTree>,
    pub(super) flat_items: Vec<Item>,

    // UI state
    pub(super) cursor: usize,
    pub(super) selected_session: Option<String>,
    pub(super) selected_group: Option<String>,
    /// Which profile the selected group belongs to (for scoped group operations)
    pub(super) selected_group_profile: Option<String>,
    pub(super) view_mode: ViewMode,
    pub(super) sort_order: SortOrder,
    pub(super) group_by: GroupByMode,
    /// Collapsed state for project-mode groups (persists across rebuilds)
    pub(super) project_group_collapsed: HashMap<String, bool>,

    // Dialogs
    pub(super) show_help: bool,
    pub(super) new_dialog: Option<NewSessionDialog>,
    pub(super) confirm_dialog: Option<ConfirmDialog>,
    pub(super) unified_delete_dialog: Option<UnifiedDeleteDialog>,
    pub(super) group_delete_options_dialog: Option<GroupDeleteOptionsDialog>,
    pub(super) rename_dialog: Option<RenameDialog>,
    pub(super) group_rename_context: Option<GroupRenameContext>,
    pub(super) hook_trust_dialog: Option<HookTrustDialog>,
    /// Session data pending hook trust approval
    pub(super) pending_hook_trust_data: Option<NewSessionData>,
    pub(super) hooks_install_dialog: Option<HooksInstallDialog>,
    /// Session data pending agent hooks acknowledgment
    pub(super) pending_hooks_install_data: Option<NewSessionData>,
    pub(super) welcome_dialog: Option<WelcomeDialog>,
    pub(super) no_agents_dialog: Option<NoAgentsDialog>,
    pub(super) changelog_dialog: Option<ChangelogDialog>,
    pub(super) info_dialog: Option<InfoDialog>,
    pub(super) profile_picker_dialog: Option<ProfilePickerDialog>,
    pub(super) projects_dialog: Option<ProjectsDialog>,
    pub(super) command_palette: Option<CommandPaletteDialog>,
    #[cfg(feature = "serve")]
    pub(super) serve_view: Option<ServeView>,
    pub(super) update_confirm_dialog: Option<UpdateConfirmDialog>,
    pub(super) send_message_dialog: Option<super::dialogs::SendMessageDialog>,
    /// Session to receive the message from the send dialog
    pub(super) pending_send_session: Option<String>,
    /// Pasted text captured at the home view that we couldn't immediately
    /// route (no session selected, cursor on a group header, etc.). Drained
    /// into the next compose dialog the user opens, so voice/dictation never
    /// gets thrown on the floor with a scolding info dialog.
    pub(super) pending_paste: Option<String>,
    /// Session to attach after the custom instruction warning dialog is dismissed
    pub(super) pending_attach_after_warning: Option<String>,
    /// Session to stop after the confirmation dialog is accepted
    pub(super) pending_stop_session: Option<String>,
    /// Session to force-remove after the confirmation dialog is accepted
    pub(super) pending_force_remove_session: Option<String>,
    // Search
    pub(super) search_active: bool,
    pub(super) search_query: Input,
    pub(super) search_matches: Vec<usize>,
    pub(super) search_match_index: usize,

    // Tool availability
    pub(super) available_tools: AvailableTools,

    // Performance: background status polling
    pub(super) status_poller: StatusPoller,
    pub(super) pending_status_refresh: bool,

    // Performance: background deletion
    pub(super) deletion_poller: DeletionPoller,

    // Performance: background session creation (for sandbox)
    pub(super) creation_poller: CreationPoller,
    /// Set to true if user cancelled while creation was pending
    pub(super) creation_cancelled: bool,
    /// Sessions whose on_launch hooks already ran in the creation poller
    pub(super) on_launch_hooks_ran: HashSet<String>,

    /// Hook progress for sessions in Creating state, keyed by stub instance ID
    pub(super) creating_hook_progress: HashMap<String, CreatingHookProgress>,
    /// The stub instance ID for the current background creation
    pub(super) creating_stub_id: Option<String>,

    // Performance: preview caching
    pub(super) preview_cache: PreviewCache,
    pub(super) terminal_preview_cache: PreviewCache,
    pub(super) container_terminal_preview_cache: PreviewCache,
    pub(super) tool_preview_cache: PreviewCache,

    /// Mouse wheel offset for the preview pane, in lines back from the bottom.
    /// Reset to 0 whenever the selected session changes.
    pub(super) preview_scroll_offset: u16,
    pub(super) preview_area: Rect,
    pub(super) diff_area: Rect,

    // Terminal mode for sandboxed sessions (per-session, ephemeral)
    pub(super) terminal_modes: HashMap<String, TerminalMode>,
    // Default terminal mode from config
    pub(super) default_terminal_mode: TerminalMode,

    // Sound config for state transition sounds
    pub(super) sound_config: crate::sound::SoundConfig,
    pub(super) status_hook_config: crate::status_hooks::StatusHookConfig,
    pub(super) status_hook_configs: HashMap<String, crate::status_hooks::StatusHookConfig>,

    /// Resolved decay window from `Config.theme.idle_decay_minutes`. Read
    /// at startup and re-resolved on settings reload. Used by render to
    /// drive the breathe rattle and fresh-idle color, and by the `w`
    /// keybind to gate which Idle sessions are still "actionable".
    pub(super) idle_decay_window: std::time::Duration,

    // When true, letter-based action hotkeys require SHIFT (guard against
    // dictation / stray keystrokes triggering destructive actions).
    pub(super) strict_hotkeys: bool,

    // Settings view
    pub(super) settings_view: Option<SettingsView>,
    /// Flag to indicate we're confirming settings close (unsaved changes)
    pub(super) settings_close_confirm: bool,

    // Diff view
    pub(super) diff_view: Option<DiffView>,

    // Resizable list column width (percentage-like units)
    pub(super) list_width: u16,

    /// Channel that startup-recovery workers send results back on. `None`
    /// when no recovery was attempted at construction (live tmux, daemon
    /// owns recovery, lock contended, or no candidates). Drained on every
    /// tick by `apply_recovery_updates`.
    recovery_rx: Option<std::sync::mpsc::Receiver<RecoveryUpdate>>,
    /// Lock guard kept alive for the recovery pass so a peer (a daemon
    /// that starts after the TUI) cannot duplicate cascades. Released
    /// when the field is set to `None` after the last worker has
    /// reported back.
    recovery_lock: Option<crate::session::recovery::RecoveryLock>,

    /// Ids whose startup-recovery cascade is still in flight. Filtered
    /// out of `request_status_refresh` so the 500ms poller does not
    /// observe missing tmux state and broadcast `Status::Error` while a
    /// worker is mid-cascade. Drained per-id by `apply_recovery_updates`
    /// (success, error, or panic). Mirrors the `on_launch_hooks_ran`
    /// HashSet pattern: TUI-local, event-driven, no TTL needed.
    recovery_in_flight: std::collections::HashSet<String>,

    // Tool sessions config (lazygit, yazi, etc.)
    pub(super) tool_configs: HashMap<String, crate::session::config::ToolSessionConfig>,
    /// Pre-parsed and sorted view of valid tool hotkeys: (name, KeyCode, KeyModifiers).
    /// Built once at construction and on settings reload, then iterated on every
    /// keystroke to look up matching tools. Sorted by name so the alphabetically-first
    /// tool wins on duplicate hotkeys.
    pub(super) tool_hotkey_cache: Vec<(
        String,
        crossterm::event::KeyCode,
        crossterm::event::KeyModifiers,
    )>,
    pub(super) tool_picker_dialog: Option<super::dialogs::ToolPickerDialog>,
}

impl HomeView {
    pub fn new(
        active_profile: Option<String>,
        available_tools: AvailableTools,
    ) -> anyhow::Result<Self> {
        use crate::session::list_profiles;

        let mut storages = HashMap::new();
        let mut all_instances = Vec::new();
        let mut group_trees = HashMap::new();

        let profile_names = match &active_profile {
            Some(name) => vec![name.clone()],
            None => list_profiles()?.into_iter().collect(),
        };

        for profile_name in &profile_names {
            let storage = Storage::new(profile_name)?;
            let (mut instances, groups) = storage.load_with_groups()?;
            for inst in &mut instances {
                inst.source_profile = profile_name.clone();
            }
            let tree = GroupTree::new_with_groups(&instances, &groups);
            group_trees.insert(profile_name.clone(), tree);
            all_instances.extend(instances);
            storages.insert(profile_name.clone(), storage);
        }

        let instance_map: HashMap<String, Instance> = all_instances
            .iter()
            .map(|i| (i.id.clone(), i.clone()))
            .collect();

        // In unified mode, config comes from "default" profile
        let config_profile = active_profile.as_deref().unwrap_or("default");
        let resolved = resolve_config_or_warn(config_profile);
        let default_terminal_mode = match resolved.sandbox.default_terminal_mode {
            DefaultTerminalMode::Host => TerminalMode::Host,
            DefaultTerminalMode::Container => TerminalMode::Container,
        };
        let sound_config = resolved.sound.clone();
        let status_hook_configs = Self::load_status_hook_configs(Self::status_hook_profile_names(
            active_profile.as_deref(),
            &storages,
        ));
        let status_hook_config = status_hook_configs
            .get(config_profile)
            .cloned()
            .unwrap_or_else(|| resolved.status_hooks.clone());
        let strict_hotkeys = resolved.session.strict_hotkeys;
        let idle_decay_window =
            crate::tui::styles::idle_decay_window(resolved.theme.idle_decay_minutes);
        let user_config = load_config().ok().flatten();
        let sort_order = user_config
            .as_ref()
            .and_then(|c| c.app_state.sort_order)
            .unwrap_or_default();
        // New users (haven't dismissed the welcome screen) default to Project
        // grouping so they see the same layout as the web dashboard. Existing
        // users keep Manual (the existing behavior) unless they explicitly
        // toggle to Project with `g`.
        let is_new_user = user_config
            .as_ref()
            .is_none_or(|c| !c.app_state.has_seen_welcome);
        let default_group_by = if is_new_user {
            GroupByMode::Project
        } else {
            GroupByMode::Manual
        };
        let group_by = user_config
            .as_ref()
            .and_then(|c| c.app_state.group_by)
            .unwrap_or(default_group_by);

        let mut view = Self {
            storages,
            active_profile,
            instances: all_instances,
            instance_map,
            group_trees,
            flat_items: Vec::new(),
            cursor: 0,
            selected_session: None,
            selected_group: None,
            selected_group_profile: None,
            view_mode: ViewMode::default(),
            sort_order,
            group_by,
            project_group_collapsed: HashMap::new(),
            show_help: false,
            new_dialog: None,
            confirm_dialog: None,
            unified_delete_dialog: None,
            group_delete_options_dialog: None,
            rename_dialog: None,
            group_rename_context: None,
            hook_trust_dialog: None,
            pending_hook_trust_data: None,
            hooks_install_dialog: None,
            pending_hooks_install_data: None,
            welcome_dialog: None,
            no_agents_dialog: None,
            changelog_dialog: None,
            info_dialog: None,
            profile_picker_dialog: None,
            projects_dialog: None,
            command_palette: None,
            #[cfg(feature = "serve")]
            serve_view: None,
            update_confirm_dialog: None,
            send_message_dialog: None,
            pending_send_session: None,
            pending_paste: None,
            pending_attach_after_warning: None,
            pending_stop_session: None,
            pending_force_remove_session: None,
            search_active: false,
            search_query: Input::default(),
            search_matches: Vec::new(),
            search_match_index: 0,
            available_tools,
            status_poller: StatusPoller::new(),
            pending_status_refresh: false,
            deletion_poller: DeletionPoller::new(),
            creation_poller: CreationPoller::new(),
            creation_cancelled: false,
            on_launch_hooks_ran: HashSet::new(),
            creating_hook_progress: HashMap::new(),
            creating_stub_id: None,
            preview_cache: PreviewCache::default(),
            terminal_preview_cache: PreviewCache::default(),
            container_terminal_preview_cache: PreviewCache::default(),
            tool_preview_cache: PreviewCache::default(),
            preview_scroll_offset: 0,
            preview_area: Rect::default(),
            diff_area: Rect::default(),
            terminal_modes: HashMap::new(),
            default_terminal_mode,
            sound_config,
            status_hook_config,
            status_hook_configs,
            strict_hotkeys,
            idle_decay_window,
            settings_view: None,
            settings_close_confirm: false,
            diff_view: None,
            list_width: user_config
                .as_ref()
                .and_then(|c| c.app_state.home_list_width)
                .unwrap_or(35),
            recovery_rx: None,
            recovery_lock: None,
            recovery_in_flight: std::collections::HashSet::new(),
            tool_configs: user_config
                .as_ref()
                .map(|c| c.tools.clone())
                .unwrap_or_default(),
            tool_hotkey_cache: Vec::new(),
            tool_picker_dialog: None,
        };

        view.tool_hotkey_cache = input::build_tool_hotkey_cache(&view.tool_configs);
        let hotkey_warnings = input::validate_tool_hotkeys(&view.tool_configs);
        if !hotkey_warnings.is_empty() && view.info_dialog.is_none() {
            view.info_dialog = Some(InfoDialog::new(
                "Tool hotkey config errors",
                &hotkey_warnings.join("\n"),
            ));
        }

        // Clean up orphaned Creating instances from a prior crash
        let orphan_ids: Vec<String> = view
            .instances
            .iter()
            .filter(|i| i.status == crate::session::Status::Creating)
            .map(|i| i.id.clone())
            .collect();
        for id in &orphan_ids {
            view.remove_instance(id);
        }
        if !orphan_ids.is_empty() {
            tracing::info!(target: "tui.home", "Cleaned up {} orphaned creating sessions", orphan_ids.len());
            if let Err(e) = view.save() {
                tracing::warn!(target: "tui.home", "Failed to save view state: {e}");
            }
        }

        // Batch-sync instance IDs and captured session IDs to tmux hidden env
        // so that build_exclusion_set() on other AoE instances can see them.
        {
            let mut set_batch: Vec<(String, String, String)> = Vec::new();
            let mut unset_batch: Vec<(String, String)> = Vec::new();
            for inst in &view.instances {
                let tmux_name = match inst.tmux_session() {
                    Ok(s) if s.exists() && !s.is_pane_dead() => s.name().to_string(),
                    _ => continue,
                };

                set_batch.push((
                    tmux_name.clone(),
                    crate::tmux::env::AOE_INSTANCE_ID_KEY.to_string(),
                    inst.id.clone(),
                ));
                if let Some(ref sid) = inst.agent_session_id {
                    set_batch.push((
                        tmux_name,
                        crate::tmux::env::AOE_CAPTURED_SESSION_ID_KEY.to_string(),
                        sid.clone(),
                    ));
                } else {
                    unset_batch.push((
                        tmux_name,
                        crate::tmux::env::AOE_CAPTURED_SESSION_ID_KEY.to_string(),
                    ));
                }
            }
            if !set_batch.is_empty() {
                let batch_refs: Vec<(&str, &str, &str)> = set_batch
                    .iter()
                    .map(|(s, k, v)| (s.as_str(), k.as_str(), v.as_str()))
                    .collect();
                if let Err(e) = crate::tmux::env::set_hidden_env_batch(&batch_refs) {
                    tracing::warn!(target: "tui.home", "Batch env sync failed: {}", e);
                }
            }
            if !unset_batch.is_empty() {
                let batch_refs: Vec<(&str, &str)> = unset_batch
                    .iter()
                    .map(|(s, k)| (s.as_str(), k.as_str()))
                    .collect();
                if let Err(e) = crate::tmux::env::remove_hidden_env_batch(&batch_refs) {
                    tracing::warn!(target: "tui.home", "Batch env unset failed: {}", e);
                }
            }
        }

        // Recover session IDs for pre-existing sessions via pollers.
        for inst in &mut view.instances {
            let has_live_tmux = inst.has_live_tmux_pane();
            if !has_live_tmux {
                continue;
            }

            if inst.supports_session_poller() && inst.session_id_poller.is_none() {
                inst.maybe_start_poller();
            }
        }

        // Startup auto-recovery: kick off a worker pool to restart any
        // resume-capable sessions whose tmux pane is missing. The TUI defers
        // to the daemon when one is running (the daemon owns recovery in
        // that case); when the TUI is standalone, it acquires the
        // cross-process recovery lock to keep a late-starting daemon from
        // duplicating cascades. See `crate::session::recovery` for the full
        // exclusion rationale.
        view.maybe_start_startup_recovery();

        view.instance_map = view
            .instances
            .iter()
            .map(|i| (i.id.clone(), i.clone()))
            .collect();

        view.flat_items = view.build_flat_items();
        view.update_selected();
        Ok(view)
    }

    pub fn reload(&mut self) -> anyhow::Result<()> {
        use crate::session::list_profiles;

        let mut all_instances = Vec::new();

        // Re-discover profiles in "all" mode
        if self.active_profile.is_none() {
            let current_profiles = list_profiles()?;
            for name in &current_profiles {
                if !self.storages.contains_key(name) {
                    self.storages.insert(name.clone(), Storage::new(name)?);
                }
            }
            self.storages.retain(|k, _| current_profiles.contains(k));
        }
        self.refresh_status_hook_config_cache();

        for (profile_name, storage) in &self.storages {
            let (mut instances, groups) = storage.load_with_groups()?;
            for inst in &mut instances {
                inst.source_profile = profile_name.clone();
                if let Some(prev) = self.instance_map.get(&inst.id) {
                    inst.status = prev.status;
                    inst.last_error = prev.last_error.clone();
                    inst.last_error_check = prev.last_error_check;
                    inst.last_start_time = prev.last_start_time;
                    inst.session_id_poller = prev.session_id_poller.clone();
                    // Carry the in-memory idle_entered_at across reloads
                    // so a freshly-stopped session doesn't lose its
                    // freshness state when the user toggles a setting
                    // that triggers a reload mid-window.
                    inst.idle_entered_at = prev.idle_entered_at;
                    // Use in-memory session_id if present; fallback to disk.
                    // In-memory state takes priority over disk: the poller
                    // may have updated the ID since last save.
                    inst.agent_session_id = prev
                        .agent_session_id
                        .clone()
                        .or(inst.agent_session_id.take());
                    // Carry the resume-fallback exclusion set across
                    // reloads. Without this, a stale sid that the cascade
                    // just cleared would be re-imported on the next 5s reload
                    // (the on-disk session artifact persists for ~5-10
                    // min after the agent's crash). The set is
                    // `#[serde(skip)]` runtime-only so disk reloads
                    // would otherwise reset it to empty.
                    inst.retroactive_capture_excludes = prev.retroactive_capture_excludes.clone();
                }
            }
            // Rebuild this profile's tree from disk, preserving any collapsed
            // state that was toggled in-memory but not yet on disk
            let mut new_tree = GroupTree::new_with_groups(&instances, &groups);
            if let Some(old_tree) = self.group_trees.get(profile_name) {
                for g in old_tree.get_all_groups() {
                    if g.collapsed {
                        new_tree.set_collapsed(&g.path, true);
                    }
                }
            }
            self.group_trees.insert(profile_name.clone(), new_tree);
            all_instances.extend(instances);
        }

        // Remove trees for profiles that no longer exist
        let storage_keys: Vec<String> = self.storages.keys().cloned().collect();
        self.group_trees.retain(|k, _| storage_keys.contains(k));

        self.instances = all_instances;

        // Re-inject any in-flight Creating stub that won't be on disk
        if let Some(ref stub_id) = self.creating_stub_id {
            if !self.instances.iter().any(|i| i.id == *stub_id) {
                if let Some(stub) = self.instance_map.get(stub_id).cloned() {
                    self.instances.push(stub);
                }
            }
        }

        self.instance_map = self
            .instances
            .iter()
            .map(|i| (i.id.clone(), i.clone()))
            .collect();
        // Remember what the cursor was pointing at so we can follow it
        let prev_selected_session = self.selected_session.clone();
        let prev_selected_group = self.selected_group.clone();

        self.flat_items = self.build_flat_items();

        // Try to restore cursor to the same session/group after rebuild
        let mut restored = false;
        if let Some(ref sid) = prev_selected_session {
            for (idx, item) in self.flat_items.iter().enumerate() {
                if let Item::Session { id, .. } = item {
                    if id == sid {
                        self.cursor = idx;
                        restored = true;
                        break;
                    }
                }
            }
        } else if let Some(ref gpath) = prev_selected_group {
            for (idx, item) in self.flat_items.iter().enumerate() {
                if let Item::Group { path, .. } = item {
                    if path == gpath {
                        self.cursor = idx;
                        restored = true;
                        break;
                    }
                }
            }
        }
        if !restored && self.cursor >= self.flat_items.len() && !self.flat_items.is_empty() {
            self.cursor = self.flat_items.len() - 1;
        }

        if self.search_active && !self.search_query.value().is_empty() {
            self.update_search();
        } else if !self.search_matches.is_empty() {
            // Recalculate match indices without moving the cursor
            self.refresh_search_matches();
        }

        self.update_selected();
        Ok(())
    }

    /// Snapshot of `self.instances` eligible for status polling.
    /// In-flight recovery candidates are excluded; their post-cascade
    /// `Instance` arrives via `apply_recovery_updates` and skipping the
    /// parallel poll prevents racing transitions during the suppression
    /// window.
    pub(super) fn pollable_instances(&self) -> Vec<Instance> {
        self.instances
            .iter()
            .filter(|i| !self.recovery_in_flight.contains(&i.id))
            .cloned()
            .collect()
    }

    pub(super) fn attached_status_hook_sessions(
        &self,
    ) -> Vec<super::attached_status_hooks::AttachedStatusHookSession> {
        self.pollable_instances()
            .into_iter()
            .filter_map(|instance| {
                let hook_config = self.status_hook_config_for(&instance);
                hook_config.enabled.then_some(
                    super::attached_status_hooks::AttachedStatusHookSession {
                        instance,
                        hook_config,
                    },
                )
            })
            .collect()
    }

    /// Request a status refresh in the background (non-blocking).
    /// Call `apply_status_updates` to check for and apply results.
    pub fn request_status_refresh(&mut self) {
        if !self.pending_status_refresh {
            self.status_poller
                .request_refresh(self.pollable_instances());
            self.pending_status_refresh = true;
        }
    }

    /// Apply any pending status updates from the background poller.
    /// Returns true if updates were applied.
    pub fn apply_status_updates(&mut self) -> bool {
        if let Some(updates) = self.status_poller.try_recv_updates() {
            for update in updates {
                self.apply_one_status_update(update);
            }
            self.pending_status_refresh = false;
            return true;
        }
        false
    }

    /// Apply a single status update from the poller. Extracted from the
    /// channel-pulling loop in `apply_status_updates` so tests can drive
    /// the apply path directly without having to push through the
    /// background polling thread.
    pub(super) fn apply_one_status_update(&mut self, update: StatusUpdate) {
        self.apply_status_update(update, true, true);
    }

    pub(super) fn apply_status_updates_without_hooks(&mut self, updates: Vec<StatusUpdate>) {
        for update in updates {
            self.apply_status_update(update, false, false);
        }
    }

    pub(super) fn reset_status_refresh(&mut self) {
        self.status_poller = StatusPoller::new();
        self.pending_status_refresh = false;
    }

    fn apply_status_update(&mut self, update: StatusUpdate, play_sound: bool, run_hooks: bool) {
        use crate::session::Status;

        let old_status = self.get_instance(&update.id).map(|i| i.status);
        let should_update = old_status.is_some_and(|s| {
            s != Status::Deleting
                && s != Status::Creating
                && s != Status::Stopped
                && update.status != Status::Stopped
        });
        if !should_update {
            return;
        }

        let new_status = update.status;
        let new_error = update.last_error;
        let new_idle_entered_at = update.idle_entered_at;
        self.mutate_instance(&update.id, |inst| {
            inst.status = new_status;
            inst.last_error = new_error;
            // Propagate the timestamp the polling clone wrote;
            // see StatusPoller for why this isn't a simple
            // `inst.idle_entered_at = …` from inside the poll.
            inst.idle_entered_at = new_idle_entered_at;
        });

        if let Some(old) = old_status {
            if old != new_status {
                if let Some(inst) = self.get_instance(&update.id).cloned() {
                    self.handle_status_transition(&inst, old, new_status, play_sound, run_hooks);
                }
            }
        }
    }

    fn handle_status_transition(
        &self,
        inst: &Instance,
        old: crate::session::Status,
        new: crate::session::Status,
        play_sound: bool,
        run_hooks: bool,
    ) {
        if play_sound {
            crate::sound::play_for_transition(old, new, &self.sound_config);
        }
        if run_hooks {
            let hook_config = self.status_hook_config_for(inst);
            crate::status_hooks::run_for_transition(inst, old, new, &hook_config);
        }
    }

    fn status_hook_config_for(&self, inst: &Instance) -> crate::status_hooks::StatusHookConfig {
        if self.active_profile.is_some() {
            return self.status_hook_config.clone();
        }
        let profile = inst.effective_profile();
        self.status_hook_configs
            .get(&profile)
            .cloned()
            .unwrap_or_else(|| self.status_hook_config.clone())
    }

    pub fn apply_deletion_results(&mut self) -> bool {
        use crate::session::Status;

        if let Some(result) = self.deletion_poller.try_recv_result() {
            if result.success {
                self.remove_instance(&result.session_id);
                self.rebuild_group_trees();

                if let Err(e) = self.save() {
                    tracing::error!(target: "tui.home", "Failed to save after deletion: {}", e);
                }
                if let Err(e) = self.reload() {
                    tracing::warn!(target: "tui.home", "Failed to reload session state: {e}");
                }
            } else {
                let error = if result.errors.is_empty() {
                    None
                } else {
                    Some(result.errors.join("; "))
                };
                self.mutate_instance(&result.session_id, |inst| {
                    inst.status = Status::Error;
                    inst.last_error = error;
                });
            }
            return true;
        }
        false
    }

    /// Apply any pending session ID updates from background pollers.
    /// Returns true if any instance was updated.
    pub fn apply_session_id_updates(&mut self) -> bool {
        let mut updates: Vec<(String, String)> = Vec::new();

        for inst in &self.instances {
            if let Some((_id, session_id)) = inst
                .session_id_poller
                .as_ref()
                .and_then(|p| p.lock().ok())
                .and_then(|p| p.try_recv_session_update())
            {
                let Some(session_id) = crate::session::capture::validated_session_id(session_id)
                else {
                    continue;
                };
                // Defense-in-depth against the resume-fallback cascade: a sid
                // the cascade just cleared can still live on disk for several
                // minutes (opencode db, vibe meta.json, codex/gemini/pi/hermes
                // state). The poller closures filter via `compose_exclusion`,
                // but if a closure factory ever forgets to thread the per-
                // instance excludes, this guard prevents the cleared sid from
                // being re-imported into memory and disk.
                if inst.retroactive_capture_excludes.contains(&session_id) {
                    tracing::debug!(
                        target: "tui.home",
                        "Ignoring poller-reported sid {} for {}: in retroactive_capture_excludes",
                        session_id,
                        inst.id,
                    );
                    continue;
                }
                if inst.agent_session_id.as_deref() != Some(session_id.as_str()) {
                    updates.push((inst.id.clone(), session_id));
                }
                continue;
            }
        }

        if !updates.is_empty() {
            let prev: Vec<(String, Option<String>)> = updates
                .iter()
                .filter_map(|(id, _)| {
                    self.get_instance(id)
                        .map(|inst| (id.clone(), inst.agent_session_id.clone()))
                })
                .collect();

            for (id, session_id) in &updates {
                self.mutate_instance(id, |inst| {
                    inst.agent_session_id = Some(session_id.clone());
                });
            }
            if let Err(e) = self.save() {
                tracing::error!(target: "tui.home", "Failed to save after session ID update: {}", e);
                for (id, old_val) in &prev {
                    self.mutate_instance(id, |inst| {
                        inst.agent_session_id = old_val.clone();
                    });
                }
                return false;
            }
        }
        !updates.is_empty()
    }

    /// Drain the startup-recovery channel and apply each `RecoveryUpdate`
    /// to the in-memory `Instance` snapshot. Released the recovery lock
    /// (and the receiver) when all workers have completed.
    ///
    /// Called from the `App::run` event-loop tick alongside
    /// `apply_session_id_updates`. Returns true if any instance was
    /// touched, so the caller can refresh the rendered tree.
    pub fn apply_recovery_updates(&mut self) -> bool {
        let Some(rx) = self.recovery_rx.as_ref() else {
            return false;
        };
        let mut touched = false;
        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok(update) => {
                    let RecoveryUpdate {
                        instance_id,
                        title,
                        instance,
                        result,
                    } = update;
                    match result {
                        Ok(crate::session::StartOutcome::Resumed) => {
                            tracing::info!(
                                target: "session.startup_recovery",
                                id = %instance_id,
                                %title,
                                "resumed",
                            );
                        }
                        Ok(crate::session::StartOutcome::Restarted { stale_sid }) => {
                            tracing::warn!(
                                target: "session.startup_recovery",
                                id = %instance_id,
                                %title,
                                %stale_sid,
                                "restarted fresh after resume failure",
                            );
                        }
                        Ok(crate::session::StartOutcome::Fresh) => {}
                        Err(e) => {
                            tracing::warn!(
                                target: "session.startup_recovery",
                                id = %instance_id,
                                %title,
                                error = %e,
                                "recovery cascade failed",
                            );
                        }
                    }
                    // Drop the in-flight marker BEFORE replacing the
                    // snapshot so the next status poll sees the post-cascade
                    // instance through the normal pipeline.
                    self.recovery_in_flight.remove(&instance_id);
                    if let Some(slot) = self.instances.iter_mut().find(|i| i.id == instance_id) {
                        *slot = *instance;
                        touched = true;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if disconnected {
            // All workers exited: drop the receiver and the lock so a
            // peer (a daemon that just started) can run recovery for any
            // session this TUI did not own. Clear the in-flight set
            // defensively in case a future early-return path bypassed
            // the per-id remove above.
            self.recovery_rx = None;
            self.recovery_lock = None;
            self.recovery_in_flight.clear();
        }
        if touched {
            self.instance_map = self
                .instances
                .iter()
                .map(|i| (i.id.clone(), i.clone()))
                .collect();
            // Preserve the selection across the rebuild. Without this, a
            // recovery completion that reorders rows under
            // `SortOrder::LastActivity` (the recovered session's
            // `last_start_time` shifted) would silently latch the
            // selection onto a neighbour because `update_selected()`
            // resolves through `flat_items[cursor]`. Mirrors the
            // canonical sequence in `reload()`.
            let prev_selected_session = self.selected_session.clone();
            let prev_selected_group = self.selected_group.clone();

            self.flat_items = self.build_flat_items();

            let mut restored = false;
            if let Some(ref sid) = prev_selected_session {
                for (idx, item) in self.flat_items.iter().enumerate() {
                    if let Item::Session { id, .. } = item {
                        if id == sid {
                            self.cursor = idx;
                            restored = true;
                            break;
                        }
                    }
                }
            } else if let Some(ref gpath) = prev_selected_group {
                for (idx, item) in self.flat_items.iter().enumerate() {
                    if let Item::Group { path, .. } = item {
                        if path == gpath {
                            self.cursor = idx;
                            restored = true;
                            break;
                        }
                    }
                }
            }
            if !restored && self.cursor >= self.flat_items.len() && !self.flat_items.is_empty() {
                self.cursor = self.flat_items.len() - 1;
            }

            if self.search_active && !self.search_query.value().is_empty() {
                self.update_search();
            } else if !self.search_matches.is_empty() {
                self.refresh_search_matches();
            }

            self.update_selected();
        }
        touched
    }

    /// Identify recovery candidates and spawn a worker pool. Sets
    /// `self.recovery_rx` to `Some(rx)` if at least one worker was spawned;
    /// otherwise leaves it `None` (the daemon owns recovery, the lock is
    /// contended, or there are no candidates).
    fn maybe_start_startup_recovery(&mut self) {
        // Requires a tokio runtime: each worker is `tokio::spawn`-ed below.
        // `HomeView::new` is sync and called from production via
        // `#[tokio::main]`, so the runtime is present at the real call site.
        // Unit tests construct `HomeView` directly without a runtime; today
        // they do not panic only because their test instances lack a valid
        // `agent_session_id` and `is_recovery_candidate` filters them out
        // before any spawn is attempted. This guard makes the function
        // resilient to a future test that constructs an instance with a
        // valid sid and no live tmux.
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        // Defer to the daemon if one is running. The daemon's own
        // `daemon_startup_recovery` will handle the candidates from this
        // TUI's profile (and every other profile). Recovery split-brain
        // is the exact failure mode the file lock is meant to prevent;
        // checking `daemon_pid()` first short-circuits the more expensive
        // lock acquisition in the common case.
        #[cfg(feature = "serve")]
        if crate::cli::serve::daemon_pid().is_some() {
            return;
        }
        let lock = match crate::session::recovery::try_acquire_recovery_lock() {
            Ok(Some(l)) => l,
            Ok(None) => {
                tracing::info!(
                    target: "session.startup_recovery",
                    "another process holds the recovery lock; TUI skipping startup recovery",
                );
                return;
            }
            Err(e) => {
                tracing::warn!(
                    target: "session.startup_recovery",
                    error = %e,
                    "failed to acquire recovery lock; TUI skipping startup recovery",
                );
                return;
            }
        };

        let mut candidates: Vec<crate::session::Instance> = Vec::new();
        // Single fallible tmux probe instead of per-instance
        // `inst.has_live_tmux_pane()` calls. On Err: skip recovery this
        // launch (a transient tmux glitch must NOT collapse to "all panes
        // dead" and trigger phantom cascades). Bonus: one subprocess call
        // regardless of instance count (was 1-2 per instance).
        let pane_meta = match crate::tmux::batch_pane_metadata() {
            Ok(map) => map,
            Err(e) => {
                tracing::warn!(
                    target: "session.startup_recovery",
                    error = %e,
                    "tmux probe failed; TUI skipping startup recovery this launch",
                );
                drop(lock);
                return;
            }
        };
        for inst in &mut self.instances {
            let session_name = crate::tmux::Session::generate_name(&inst.id, &inst.title);
            let has_live_tmux = pane_meta
                .get(&session_name)
                .map(|m| !m.pane_dead)
                .unwrap_or(false);
            if has_live_tmux {
                continue;
            }
            if !crate::session::recovery::is_recovery_candidate(inst) {
                continue;
            }
            // Set Status::Starting AND last_start_time: the existing 3s
            // grace at `update_status_with_metadata_inner` only fires on
            // the latter, and without it the TUI's StatusPoller (every
            // 500ms) would observe missing tmux + no last_start_time and
            // immediately flip the status to `Error` before the worker
            // has finished its cascade.
            debug_assert!(inst.status != crate::session::Status::Creating);
            inst.status = crate::session::Status::Starting;
            inst.last_error = None;
            inst.last_start_time = Some(std::time::Instant::now());
            self.recovery_in_flight.insert(inst.id.clone());
            candidates.push(inst.clone());
        }

        if candidates.is_empty() {
            drop(lock);
            return;
        }

        crate::session::recovery::warm_tmux_server();

        tracing::info!(
            target: "session.startup_recovery",
            count = candidates.len(),
            "TUI starting recovery for missing tmux sessions",
        );

        let (tx, rx) = std::sync::mpsc::channel::<RecoveryUpdate>();
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(
            crate::session::recovery::STARTUP_RECOVERY_CONCURRENCY,
        ));

        for inst in candidates {
            let tx = tx.clone();
            let permit_sem = semaphore.clone();
            tokio::spawn(async move {
                let _permit = permit_sem
                    .acquire_owned()
                    .await
                    .expect("recovery semaphore not closed");
                let id = inst.id.clone();
                let title = inst.title.clone();
                let inst_pre_panic = inst.clone();
                let mut working = inst;
                let result = tokio::task::spawn_blocking(move || {
                    let res = crate::session::recovery::run_recovery_for_instance(&mut working);
                    (working, res)
                })
                .await;
                let update = match result {
                    Ok((updated, Ok(outcome))) => RecoveryUpdate {
                        instance_id: id,
                        title,
                        instance: Box::new(updated),
                        result: Ok(outcome),
                    },
                    Ok((updated, Err(e))) => RecoveryUpdate {
                        instance_id: id,
                        title,
                        instance: Box::new(updated),
                        result: Err(e.to_string()),
                    },
                    Err(join_err) => {
                        tracing::error!(
                            target: "session.startup_recovery",
                            id = %id,
                            error = %join_err,
                            "recovery worker panicked",
                        );
                        // Surface the panic as a synthetic error update so
                        // `apply_recovery_updates` clears `recovery_in_flight`
                        // and the user sees Status::Error with a useful
                        // last_error instead of an instance stuck in
                        // `Status::Starting` until HomeView drops.
                        let mut recovered = inst_pre_panic;
                        recovered.status = crate::session::Status::Error;
                        recovered.last_error =
                            Some(format!("recovery worker panicked: {}", join_err));
                        RecoveryUpdate {
                            instance_id: id,
                            title,
                            instance: Box::new(recovered),
                            result: Err(format!("worker panicked: {}", join_err)),
                        }
                    }
                };
                let _ = tx.send(update);
            });
        }

        self.recovery_rx = Some(rx);
        self.recovery_lock = Some(lock);
    }

    /// Request background session creation. Used for sandbox sessions to avoid blocking UI.
    /// Creates a stub instance in the session list with Status::Creating so the user
    /// can see progress in the preview pane while continuing to use the TUI.
    pub fn request_creation(
        &mut self,
        mut data: NewSessionData,
        hooks: Option<crate::session::HooksConfig>,
    ) {
        // Pre-resolve the title using the same logic the builder will run, so the
        // stub instance, the background creation, and the eventual real instance
        // all agree on the title (otherwise an empty title would show as the path
        // basename in the stub but a civilization name in the final instance).
        if data.title.is_empty() {
            let existing_titles: Vec<&str> = self
                .instances()
                .iter()
                .filter(|i| i.source_profile == data.profile)
                .map(|i| i.title.as_str())
                .collect();
            data.title = crate::session::builder::resolve_title(
                &data.title,
                data.worktree_branch.as_deref(),
                data.worktree_enabled,
                &existing_titles,
            );
        }
        let stub_title = data.title.clone();
        let mut stub = Instance::new(&stub_title, &data.path);
        stub.tool = if data.tool.is_empty() {
            "claude".to_string()
        } else {
            data.tool.clone()
        };
        stub.group_path = data.group.clone();
        stub.status = crate::session::Status::Creating;
        stub.yolo_mode = data.yolo_mode;
        stub.source_profile = data.profile.clone();

        // Set stub worktree_info so project-mode grouping works during creation.
        // The real worktree_info (with resolved main_repo_path) replaces this
        // once build_instance completes.
        let stub_branch = data
            .worktree_branch
            .as_deref()
            .filter(|b| !b.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                data.worktree_enabled
                    .then(|| crate::session::builder::branch_name_from_title(&stub_title))
            });
        if let Some(branch) = stub_branch {
            stub.worktree_info = Some(crate::session::WorktreeInfo {
                branch,
                main_repo_path: data.path.clone(),
                managed_by_aoe: false,
                created_at: chrono::Utc::now(),
                base_branch: data.base_branch.clone(),
            });
        }

        let stub_id = stub.id.clone();
        let target_profile = data.profile.clone();

        // Add stub to instance list
        self.add_instance(stub);
        self.rebuild_group_trees();
        if !data.group.is_empty() {
            if let Some(tree) = self.group_trees.get_mut(&target_profile) {
                tree.create_group(&data.group);
            }
        }

        // Initialize progress tracking and select the stub
        self.creating_hook_progress.insert(
            stub_id.clone(),
            CreatingHookProgress {
                hook_output: Vec::new(),
                current_hook: None,
            },
        );
        self.creating_stub_id = Some(stub_id.clone());
        self.flat_items = self.build_flat_items();

        // Move cursor to the new stub
        if let Some(pos) = self
            .flat_items
            .iter()
            .position(|item| matches!(item, Item::Session { id, .. } if id == &stub_id))
        {
            self.cursor = pos;
            self.update_selected();
        }

        // Close the dialog
        self.new_dialog = None;

        self.creation_cancelled = false;
        // Filter out the stub from existing instances so the builder doesn't
        // treat its placeholder title as a duplicate to auto-increment.
        let existing_instances: Vec<Instance> = self
            .instances
            .iter()
            .filter(|i| i.id != stub_id)
            .cloned()
            .collect();
        let request = CreationRequest {
            data,
            existing_instances,
            hooks,
        };
        self.creation_poller.request_creation(request);
    }

    /// Mark the current creation operation as cancelled
    pub fn cancel_creation(&mut self) {
        if self.creation_poller.is_pending() {
            self.creation_cancelled = true;
        }
        // Remove the stub instance
        if let Some(stub_id) = self.creating_stub_id.take() {
            self.remove_instance(&stub_id);
            self.creating_hook_progress.remove(&stub_id);
            self.rebuild_group_trees();
            self.flat_items = self.build_flat_items();
            self.update_selected();
        }
        self.new_dialog = None;
    }

    /// Apply any pending creation results from the background poller.
    /// Returns Some(session_id) if creation succeeded and we should attach.
    pub fn apply_creation_results(&mut self) -> Option<String> {
        use super::creation_poller::CreationResult;
        use crate::session::builder::{self, CreatedWorktree};
        use std::path::PathBuf;

        let result = self.creation_poller.try_recv_result()?;

        // Clean up the stub and progress tracking
        let stub_id = self.creating_stub_id.take();
        if let Some(ref id) = stub_id {
            self.creating_hook_progress.remove(id);
        }

        // Check if the user cancelled while waiting
        if self.creation_cancelled {
            self.creation_cancelled = false;
            if let Some(id) = &stub_id {
                self.remove_instance(id);
            }
            if let CreationResult::Success {
                ref instance,
                ref created_worktree,
                ..
            } = result
            {
                let worktree = created_worktree.as_ref().map(|wt| CreatedWorktree {
                    path: PathBuf::from(&wt.path),
                    main_repo_path: PathBuf::from(&wt.main_repo_path),
                });
                builder::cleanup_instance(instance, worktree.as_ref(), &[]);
            }
            self.rebuild_group_trees();
            self.flat_items = self.build_flat_items();
            self.update_selected();
            return None;
        }

        match result {
            CreationResult::Success {
                session_id,
                instance,
                on_launch_hooks_ran,
                warnings,
                ..
            } => {
                // Remove the stub instance
                if let Some(id) = &stub_id {
                    self.remove_instance(id);
                }

                let mut instance = *instance;
                let target_profile = self.creation_poller.last_profile().unwrap_or_else(|| {
                    self.active_profile
                        .clone()
                        .unwrap_or_else(|| "default".to_string())
                });
                instance.source_profile = target_profile.clone();

                // Ensure target profile storage exists
                if !self.storages.contains_key(&target_profile) {
                    if let Ok(s) = Storage::new(&target_profile) {
                        self.storages.insert(target_profile.clone(), s);
                    }
                }

                self.add_instance(instance.clone());
                self.rebuild_group_trees();
                if !instance.group_path.is_empty() {
                    if let Some(tree) = self.group_trees.get_mut(&target_profile) {
                        tree.create_group(&instance.group_path);
                    }
                }

                if let Err(e) = self.save() {
                    tracing::error!(target: "tui.home", "Failed to save after creation: {}", e);
                }

                if on_launch_hooks_ran {
                    self.on_launch_hooks_ran.insert(session_id.clone());
                }

                if let Err(e) = self.reload() {
                    tracing::warn!(target: "tui.home", "Failed to reload session state: {e}");
                }
                self.new_dialog = None;

                if !warnings.is_empty() {
                    let body = warnings.join("\n\n");
                    let message = format!(
                        "Session was created, but the following warnings were emitted during worktree setup:\n\n{}",
                        body
                    );
                    // Size to fit content. The default 50x9 truncates everything
                    // past the prefix sentence, so the user only sees the prefix
                    // and a blank line. Mirrors the math in app.rs:show_startup_warning.
                    const WIDTH: u16 = 96;
                    let inner_width = WIDTH.saturating_sub(4) as usize;
                    let visual_lines: usize = message
                        .lines()
                        .map(|l| {
                            if l.is_empty() {
                                1
                            } else {
                                l.len().div_ceil(inner_width)
                            }
                        })
                        .sum();
                    let height = ((visual_lines as u16).saturating_add(7)).clamp(9, 35);
                    self.info_dialog = Some(
                        InfoDialog::new("Worktree warnings", &message).with_size(WIDTH, height),
                    );
                }

                Some(session_id)
            }
            CreationResult::Error(error) => {
                // Remove the stub and show the error in an info dialog
                if let Some(id) = &stub_id {
                    self.remove_instance(id);
                    self.rebuild_group_trees();
                    self.flat_items = self.build_flat_items();
                    self.update_selected();
                    self.info_dialog = Some(InfoDialog::new("Creation Failed", &error));
                } else if let Some(dialog) = &mut self.new_dialog {
                    dialog.set_loading(false);
                    dialog.set_error(error);
                }
                None
            }
        }
    }

    /// Check if on_launch hooks already ran for this session (and consume the flag).
    pub fn take_on_launch_hooks_ran(&mut self, session_id: &str) -> bool {
        self.on_launch_hooks_ran.remove(session_id)
    }

    /// Check if there's a pending creation operation
    pub fn is_creation_pending(&self) -> bool {
        self.creation_poller.is_pending()
    }

    /// Check if the currently selected session is the in-flight creating stub
    pub fn is_creating_stub_selected(&self) -> bool {
        match (&self.creating_stub_id, &self.selected_session) {
            (Some(stub_id), Some(selected)) => stub_id == selected,
            _ => false,
        }
    }

    /// Show a confirmation dialog warning that a session is being created.
    pub fn show_quit_during_creation_confirm(&mut self) {
        self.confirm_dialog = Some(ConfirmDialog::new(
            "Session Creating",
            "A session is still being created. Quit anyway? The hook will be cancelled.",
            "quit_during_creation",
        ));
    }

    /// Clean up a pending creation on TUI shutdown. Waits briefly for the
    /// background thread to finish so we can clean up worktrees/instances.
    /// If the thread doesn't finish in time, the hook subprocess will
    /// complete on its own and orphaned Creating stubs are cleaned up on
    /// next launch.
    pub fn cleanup_pending_creation(&mut self) {
        if !self.creation_poller.is_pending() {
            return;
        }
        self.creation_cancelled = true;
        if let Some(stub_id) = self.creating_stub_id.take() {
            self.remove_instance(&stub_id);
            self.creating_hook_progress.remove(&stub_id);
        }

        // Wait briefly for the background thread to finish
        let result = self
            .creation_poller
            .recv_result_timeout(std::time::Duration::from_secs(2));

        if let Some(crate::tui::creation_poller::CreationResult::Success {
            ref instance,
            ref created_worktree,
            ..
        }) = result
        {
            let worktree =
                created_worktree
                    .as_ref()
                    .map(|wt| crate::session::builder::CreatedWorktree {
                        path: std::path::PathBuf::from(&wt.path),
                        main_repo_path: std::path::PathBuf::from(&wt.main_repo_path),
                    });
            crate::session::builder::cleanup_instance(instance, worktree.as_ref(), &[]);
            tracing::info!(target: "tui.home", "Cleaned up cancelled session on exit");
        }
    }

    /// Tick dialog animations/timers and drain hook progress.
    /// Returns true when a redraw is needed.
    pub fn tick_dialog(&mut self) -> bool {
        use crate::session::repo_config::HookProgress;

        let mut changed = false;

        if let Some(dialog) = &mut self.new_dialog {
            if dialog.tick() {
                changed = true;
            }

            if dialog.is_loading() {
                // Drain all pending hook progress messages
                while let Some(progress) = self.creation_poller.try_recv_progress() {
                    dialog.push_hook_progress(progress);
                    changed = true;
                }
            }
        }

        // Poll serve dialog for subprocess startup events.
        #[cfg(feature = "serve")]
        if let Some(view) = &mut self.serve_view {
            if view.tick() {
                changed = true;
            }
        }

        // Drain hook progress into the creating buffer when no dialog is open
        if self.new_dialog.is_none() {
            if let Some(ref stub_id) = self.creating_stub_id {
                let stub_id = stub_id.clone();
                if let Some(progress_buf) = self.creating_hook_progress.get_mut(&stub_id) {
                    while let Some(progress) = self.creation_poller.try_recv_progress() {
                        match progress {
                            HookProgress::Started(cmd) => {
                                progress_buf.current_hook = Some(cmd);
                            }
                            HookProgress::Output(line) => {
                                progress_buf.hook_output.push(line);
                                // Cap buffer to prevent unbounded memory growth
                                if progress_buf.hook_output.len() > 1000 {
                                    progress_buf.hook_output.drain(..500);
                                }
                            }
                        }
                        changed = true;
                    }
                }
            }
        }

        changed
    }

    /// Whether the user is currently looking at a surface where they're
    /// likely to want to copy text (URLs, error messages, release notes).
    /// The App uses this to release xterm mouse capture so the terminal's
    /// native drag-to-select works without a modifier; mouse capture comes
    /// back as soon as the surface is dismissed.
    ///
    /// Add new dialogs here only when their content is meant to be copied,
    /// not for every modal: capture toggling has a small but visible cost
    /// (the wheel-scroll on the dashboard preview won't work while it's
    /// off).
    pub fn wants_text_selection(&self) -> bool {
        #[cfg(feature = "serve")]
        let serve_open = self.serve_view.is_some();
        #[cfg(not(feature = "serve"))]
        let serve_open = false;

        serve_open || self.info_dialog.is_some() || self.changelog_dialog.is_some()
    }

    pub fn has_dialog(&self) -> bool {
        #[cfg(feature = "serve")]
        let serve_open = self.serve_view.is_some();
        #[cfg(not(feature = "serve"))]
        let serve_open = false;

        self.show_help
            || self.search_active
            || self.new_dialog.is_some()
            || self.confirm_dialog.is_some()
            || self.unified_delete_dialog.is_some()
            || self.group_delete_options_dialog.is_some()
            || self.rename_dialog.is_some()
            || self.hook_trust_dialog.is_some()
            || self.hooks_install_dialog.is_some()
            || self.welcome_dialog.is_some()
            || self.no_agents_dialog.is_some()
            || self.changelog_dialog.is_some()
            || self.info_dialog.is_some()
            || self.profile_picker_dialog.is_some()
            || self.projects_dialog.is_some()
            || self.command_palette.is_some()
            || self.tool_picker_dialog.is_some()
            || self.send_message_dialog.is_some()
            || self.update_confirm_dialog.is_some()
            || serve_open
            || self.settings_view.is_some()
            || self.diff_view.is_some()
    }

    /// Whether the paste-burst detector should fire for incoming key events.
    ///
    /// The detector exists to solve the home-view shortcut-shadowing problem:
    /// Mosh strips bracketed-paste markers, so a pasted stream of `KeyCode::Char`
    /// events would fire `n`/`d`/`r`/etc. shortcuts on the home view. When a
    /// dialog captures keys into a text input, those shortcuts don't fire —
    /// but the dialog also won't receive a synthesized `Paste` event unless
    /// it routes through `handle_paste`. Bursting through a dialog that only
    /// handles `Key` events strands the text in `pending_paste` and leaves
    /// the dialog's input empty.
    ///
    /// So: burst is safe when no dialog is open (home shortcuts at risk) or
    /// when one of the four paste-routed dialogs is open (rename / send_message
    /// / new / settings — each forwards to `handle_paste`). For every other
    /// dialog (command palette, profile picker, projects, info, etc.) keys
    /// must dispatch individually so the dialog input receives them.
    pub fn wants_paste_burst(&self) -> bool {
        if !self.has_dialog() {
            return true;
        }
        self.rename_dialog.is_some()
            || self.send_message_dialog.is_some()
            || self.new_dialog.is_some()
            || self.settings_view.is_some()
    }

    pub fn shrink_list(&mut self) {
        self.list_width = self.list_width.saturating_sub(5).max(10);
        self.save_list_width();
    }

    pub fn grow_list(&mut self) {
        self.list_width = (self.list_width + 5).min(80);
        self.save_list_width();
    }

    fn save_list_width(&self) {
        if let Ok(mut config) = load_config().map(|c| c.unwrap_or_default()) {
            config.app_state.home_list_width = Some(self.list_width);
            if let Err(e) = save_config(&config) {
                tracing::warn!(target: "tui.home", "Failed to save config: {e}");
            }
        }
    }

    pub fn show_welcome(&mut self) {
        tracing::info!(target: "tui.dialog", dialog = "welcome", "opening");
        self.welcome_dialog = Some(WelcomeDialog::new());
    }

    pub fn show_no_agents(&mut self) {
        tracing::info!(target: "tui.dialog", dialog = "no_agents", "opening");
        self.no_agents_dialog = Some(NoAgentsDialog::new());
    }

    /// Replace available tools (used after re-check from no-agents dialog).
    pub fn set_available_tools(&mut self, tools: AvailableTools) {
        tracing::debug!(target: "tui.home", count = tools.available_list().len(), "available tools refreshed");
        self.available_tools = tools;
    }

    pub fn show_changelog(&mut self, from_version: Option<String>) {
        tracing::info!(
            target: "tui.dialog",
            dialog = "changelog",
            from_version = ?from_version,
            "opening",
        );
        self.changelog_dialog = Some(ChangelogDialog::new(from_version));
    }

    pub fn instances(&self) -> &[Instance] {
        &self.instances
    }

    pub fn get_instance(&self, id: &str) -> Option<&Instance> {
        self.instance_map.get(id)
    }

    /// Returns true if any session has an animated status (Running, Waiting, Starting,
    /// Creating), which means the TUI needs periodic redraws for spinner animation.
    pub fn has_animated_sessions(&self) -> bool {
        use crate::session::Status;
        self.instances.iter().any(|inst| {
            matches!(
                inst.status,
                Status::Running | Status::Waiting | Status::Starting | Status::Creating
            )
        })
    }

    pub(super) fn build_flat_items(&self) -> Vec<Item> {
        if self.group_by == GroupByMode::Project {
            return self.build_flat_items_by_project();
        }

        if let Some(profile) = &self.active_profile {
            let filtered: Vec<Instance> = self
                .instances
                .iter()
                .filter(|i| i.source_profile == *profile)
                .cloned()
                .collect();
            match self.group_trees.get(profile) {
                Some(tree) => flatten_tree(tree, &filtered, self.sort_order),
                None => Vec::new(),
            }
        } else if self.storages.len() <= 1 {
            match self.group_trees.values().next() {
                Some(tree) => flatten_tree(tree, &self.instances, self.sort_order),
                None => Vec::new(),
            }
        } else {
            flatten_tree_all_profiles(&self.instances, &self.group_trees, self.sort_order)
        }
    }

    fn build_flat_items_by_project(&self) -> Vec<Item> {
        // In project mode, always merge all sessions into one tree regardless of
        // profile count. Project grouping unifies by repo across profiles.
        let base_instances: Vec<Instance> = if let Some(profile) = &self.active_profile {
            self.instances
                .iter()
                .filter(|i| i.source_profile == *profile)
                .cloned()
                .collect()
        } else {
            self.instances.clone()
        };

        let grouped: Vec<Instance> = base_instances
            .into_iter()
            .map(|mut inst| {
                inst.group_path = project_group_name(&inst);
                inst
            })
            .collect();

        let mut tree = GroupTree::new_with_groups(&grouped, &[]);
        for (path, &collapsed) in &self.project_group_collapsed {
            if collapsed {
                tree.set_collapsed(path, true);
            }
        }
        flatten_tree(&tree, &grouped, self.sort_order)
    }

    /// The active profile filter name, or `None` when no filter is applied.
    /// Returning `None` lets callers (e.g. the list-pane title) omit the
    /// `[<profile>]` segment entirely instead of rendering a noisy `[all]`.
    pub fn active_profile_display(&self) -> Option<&str> {
        self.active_profile.as_deref()
    }

    /// Switch the active profile filter in-place without destroying the view.
    /// Pass `None` for all-profiles mode, or `Some(name)` to filter to one profile.
    pub fn switch_profile(&mut self, new_profile: Option<String>) -> anyhow::Result<()> {
        self.active_profile = new_profile;
        // Clear selection before reload so stale session/group refs don't linger
        self.selected_session = None;
        self.selected_group = None;
        self.selected_group_profile = None;
        self.reload()?;
        self.refresh_from_config();
        // Invalidate preview caches since the visible sessions changed
        self.preview_cache = PreviewCache::default();
        self.terminal_preview_cache = PreviewCache::default();
        self.container_terminal_preview_cache = PreviewCache::default();
        self.tool_preview_cache = PreviewCache::default();
        self.preview_scroll_offset = 0;
        // Clear search since match indices are invalid with new flat_items
        if self.search_active {
            self.search_active = false;
            self.search_query = Input::default();
            self.search_matches.clear();
            self.search_match_index = 0;
        }
        Ok(())
    }

    /// Show the profile picker dialog with fresh data from disk.
    pub(super) fn show_profile_picker(&mut self) {
        use crate::session::list_profiles;
        use crate::tui::dialogs::{ProfileEntry, ProfilePickerDialog};

        let current_profile = self
            .active_profile
            .clone()
            .unwrap_or_else(|| "all".to_string());
        let profiles = list_profiles().unwrap_or_else(|_| vec!["default".to_string()]);
        let mut entries: Vec<ProfileEntry> = profiles
            .iter()
            .map(|name| {
                let session_count = Storage::new(name)
                    .and_then(|s| s.load())
                    .map(|instances| instances.len())
                    .unwrap_or(0);
                ProfileEntry {
                    name: name.clone(),
                    session_count,
                    is_active: self.active_profile.as_deref() == Some(name.as_str()),
                }
            })
            .collect();

        // In filtered mode, add "all" entry at top
        if self.active_profile.is_some() {
            let total: usize = entries.iter().map(|e| e.session_count).sum();
            entries.insert(
                0,
                ProfileEntry {
                    name: "all".to_string(),
                    session_count: total,
                    is_active: false,
                },
            );
        }

        self.profile_picker_dialog = Some(ProfilePickerDialog::new(entries, &current_profile));
    }

    pub fn set_instance_status(&mut self, id: &str, status: crate::session::Status) {
        let old_status = self.get_instance(id).map(|inst| inst.status);
        self.mutate_instance(id, |inst| inst.status = status);
        if let Some(old) = old_status {
            if old != status {
                if let Some(inst) = self.get_instance(id).cloned() {
                    self.handle_status_transition(&inst, old, status, false, true);
                }
            }
        }
    }

    /// Stamp `last_accessed_at` on a session (user-initiated interaction).
    pub fn stamp_last_accessed(&mut self, id: &str) {
        self.mutate_instance(id, |inst| inst.touch_last_accessed());
    }

    /// Run the send-message work after the dialog has been dismissed: call
    /// `ensure_pane_ready` (which may auto-start or respawn), then deliver
    /// the keystrokes. Errors are surfaced via `info_dialog` so the caller
    /// (`execute_action`) only has to clear its transient status.
    ///
    /// Returns `Some(stale_sid)` when the resume-fallback cascade fired
    /// during the implicit respawn so the caller can toast the user about
    /// the lost history; `None` otherwise.
    pub fn execute_send_message(&mut self, session_id: &str, message: &str) -> Option<String> {
        let outcome = self.try_mutate_instance_writeback_on_err(session_id, |inst| {
            inst.ensure_pane_ready().map_err(Into::into)
        });
        let stale_sid = match outcome {
            Ok(Some(EnsureReadyOutcome::Respawned {
                stale_sid: Some(sid),
            }))
            | Ok(Some(EnsureReadyOutcome::Started {
                stale_sid: Some(sid),
            })) => Some(sid),
            Ok(_) => None,
            Err(err) => {
                self.info_dialog = Some(InfoDialog::new(
                    "Send Failed",
                    &format!("Cannot prepare session: {}", err),
                ));
                return None;
            }
        };
        let inst = self.get_instance(session_id)?;
        let tmux_session = match crate::tmux::Session::new(&inst.id, &inst.title) {
            Ok(s) => s,
            Err(e) => {
                self.info_dialog = Some(InfoDialog::new(
                    "Send Failed",
                    &format!("Failed to resolve session: {}", e),
                ));
                return None;
            }
        };
        let delay = crate::agents::send_keys_enter_delay(&inst.tool);
        if let Err(e) = tmux_session.send_keys_with_delay(message, delay) {
            self.info_dialog = Some(InfoDialog::new(
                "Send Failed",
                &format!("Failed to send message: {}", e),
            ));
            return None;
        }
        self.stamp_last_accessed(session_id);
        stale_sid
    }

    pub fn save(&self) -> anyhow::Result<()> {
        for (profile_name, storage) in &self.storages {
            let profile_instances: Vec<Instance> = self
                .instances
                .iter()
                .filter(|i| i.source_profile == *profile_name)
                .cloned()
                .collect();
            // Each profile has its own GroupTree with correct collapsed state
            let tree = self
                .group_trees
                .get(profile_name)
                .cloned()
                .unwrap_or_else(|| GroupTree::new_with_groups(&profile_instances, &[]));
            storage.commit(&profile_instances, &tree)?;
        }
        Ok(())
    }

    /// Rebuild all per-profile GroupTrees from the current instances,
    /// preserving each tree's collapsed state.
    pub(super) fn rebuild_group_trees(&mut self) {
        for (profile_name, tree) in &mut self.group_trees {
            let existing_groups = tree.get_all_groups();
            let profile_instances: Vec<Instance> = self
                .instances
                .iter()
                .filter(|i| i.source_profile == *profile_name)
                .cloned()
                .collect();
            *tree = GroupTree::new_with_groups(&profile_instances, &existing_groups);
        }
    }

    /// Determine which profile the item at the given cursor position belongs to.
    pub(super) fn profile_for_cursor(&self, cursor: usize) -> Option<String> {
        if let Some(profile) = &self.active_profile {
            return Some(profile.clone());
        }
        if let Some(item) = self.flat_items.get(cursor) {
            match item {
                crate::session::Item::Session { id, .. } => {
                    return self
                        .get_instance(id.as_str())
                        .map(|i| i.source_profile.clone());
                }
                crate::session::Item::Group { profile, path, .. } => {
                    if let Some(p) = profile {
                        return Some(p.clone());
                    }
                    // Fallback for single-profile mode: find any instance in this group
                    return self
                        .instances
                        .iter()
                        .find(|i| {
                            i.group_path == *path || i.group_path.starts_with(&format!("{}/", path))
                        })
                        .map(|i| i.source_profile.clone());
                }
            }
        }
        None
    }

    /// Collect all groups from all per-profile GroupTrees.
    pub(super) fn all_groups(&self) -> Vec<Group> {
        self.group_trees
            .values()
            .flat_map(|t| t.get_all_groups())
            .collect()
    }

    /// Check if any profile has groups, without collecting them all.
    pub(super) fn has_any_groups(&self) -> bool {
        self.group_trees
            .values()
            .any(|t| !t.get_all_groups().is_empty())
    }

    /// Centralized instance addition: adds to both the `instances` vec
    /// and `instance_map` to keep both collections in sync.
    pub(super) fn add_instance(&mut self, instance: Instance) {
        self.instance_map
            .insert(instance.id.clone(), instance.clone());
        self.instances.push(instance);
    }

    /// Centralized instance removal: removes from both the `instances` vec
    /// and `instance_map` to keep both collections in sync.
    pub(super) fn remove_instance(&mut self, id: &str) {
        self.instances.retain(|i| i.id != id);
        self.instance_map.remove(id);
    }

    /// Centralized instance mutation: applies `f` once to the `instances` vec
    /// entry, then clones the result into `instance_map`. This guarantees both
    /// collections stay in sync even for non-idempotent closures.
    pub(super) fn mutate_instance(&mut self, id: &str, f: impl FnOnce(&mut Instance)) {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            f(inst);
            self.instance_map.insert(id.to_string(), inst.clone());
        }
    }

    /// Like `mutate_instance`, but for fallible operations. Clones the entry,
    /// applies `f` to the clone, and writes back to both collections only on
    /// success -- neither collection is modified on error.
    pub(super) fn try_mutate_instance<T>(
        &mut self,
        id: &str,
        f: impl FnOnce(&mut Instance) -> anyhow::Result<T>,
    ) -> anyhow::Result<Option<T>> {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            let mut updated = inst.clone();
            let out = f(&mut updated)?;
            *inst = updated.clone();
            self.instance_map.insert(id.to_string(), updated);
            return Ok(Some(out));
        }
        Ok(None)
    }

    /// Like `try_mutate_instance`, but writes the mutated clone back even
    /// when `f` returns `Err`.
    ///
    /// Required for callers of `Instance::restart_with_size_opts` /
    /// `ensure_pane_ready`, because the resume-fallback cascade mutates
    /// `agent_session_id` and `retroactive_capture_excludes` BEFORE
    /// returning `Err` on Tier-2 failure. The default `try_mutate_instance`
    /// drops the mutated clone on `Err`, leaving the live entry with the
    /// stale sid in memory while disk has been cleared. Subsequent restarts
    /// then loop indefinitely on the same bad sid (the TUI's `reload()`
    /// merge prefers in-memory, so even the 5s disk refresh does not
    /// recover). This helper preserves the cascade's partial mutations so
    /// the live state stays consistent with disk.
    pub(super) fn try_mutate_instance_writeback_on_err<T>(
        &mut self,
        id: &str,
        f: impl FnOnce(&mut Instance) -> anyhow::Result<T>,
    ) -> anyhow::Result<Option<T>> {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            let mut updated = inst.clone();
            let result = f(&mut updated);
            *inst = updated.clone();
            self.instance_map.insert(id.to_string(), updated);
            return result.map(Some);
        }
        Ok(None)
    }

    pub fn set_instance_error(&mut self, id: &str, error: Option<String>) {
        self.mutate_instance(id, |inst| inst.last_error = error);
    }

    pub fn start_terminal_for_instance_with_size(
        &mut self,
        id: &str,
        size: Option<(u16, u16)>,
    ) -> anyhow::Result<()> {
        self.try_mutate_instance(id, |inst| inst.start_terminal_with_size(size))?;
        self.save()?;
        Ok(())
    }

    pub fn restart_instance_with_size_opts(
        &mut self,
        id: &str,
        size: Option<(u16, u16)>,
        skip_on_launch: bool,
    ) -> anyhow::Result<crate::session::StartOutcome> {
        let outcome = self.try_mutate_instance_writeback_on_err(id, |inst| {
            inst.restart_with_size_opts(size, skip_on_launch)
        })?;
        outcome.ok_or_else(|| anyhow::anyhow!("session not found: {}", id))
    }

    pub fn select_session_by_id(&mut self, session_id: &str) {
        for (idx, item) in self.flat_items.iter().enumerate() {
            if let Item::Session { id, .. } = item {
                if id == session_id {
                    self.cursor = idx;
                    self.update_selected();
                    return;
                }
            }
        }
    }

    /// Get the terminal mode for a session (uses config default if not set)
    pub fn get_terminal_mode(&self, session_id: &str) -> TerminalMode {
        self.terminal_modes
            .get(session_id)
            .copied()
            .unwrap_or(self.default_terminal_mode)
    }

    /// Refresh all config-dependent state from the current profile's config.
    /// Call this after settings are saved to pick up any changes.
    pub fn refresh_from_config(&mut self) {
        let profile = self.active_profile.as_deref().unwrap_or("default");
        let config = resolve_config_or_warn(profile);
        self.default_terminal_mode = match config.sandbox.default_terminal_mode {
            DefaultTerminalMode::Host => TerminalMode::Host,
            DefaultTerminalMode::Container => TerminalMode::Container,
        };
        self.sound_config = config.sound.clone();
        self.status_hook_config = config.status_hooks.clone();
        self.refresh_status_hook_config_cache();
        self.strict_hotkeys = config.session.strict_hotkeys;
        self.idle_decay_window =
            crate::tui::styles::idle_decay_window(config.theme.idle_decay_minutes);
        self.tool_configs = config.tools;
        self.tool_hotkey_cache = input::build_tool_hotkey_cache(&self.tool_configs);
        let hotkey_warnings = input::validate_tool_hotkeys(&self.tool_configs);
        if !hotkey_warnings.is_empty() && self.info_dialog.is_none() {
            self.info_dialog = Some(InfoDialog::new(
                "Tool hotkey config errors",
                &hotkey_warnings.join("\n"),
            ));
        }
    }

    fn status_hook_profile_names(
        active_profile: Option<&str>,
        storages: &HashMap<String, Storage>,
    ) -> Vec<String> {
        let mut profile_names = match active_profile {
            Some(profile) => vec![profile.to_string()],
            None => storages.keys().cloned().collect(),
        };
        if !profile_names.iter().any(|profile| profile == "default") {
            profile_names.push("default".to_string());
        }
        profile_names.sort();
        profile_names.dedup();
        profile_names
    }

    fn load_status_hook_configs(
        profile_names: Vec<String>,
    ) -> HashMap<String, crate::status_hooks::StatusHookConfig> {
        profile_names
            .into_iter()
            .map(|profile| {
                let status_hooks = resolve_config_or_warn(&profile).status_hooks;
                (profile, status_hooks)
            })
            .collect()
    }

    fn refresh_status_hook_config_cache(&mut self) {
        let profile_names =
            Self::status_hook_profile_names(self.active_profile.as_deref(), &self.storages);
        self.status_hook_configs = Self::load_status_hook_configs(profile_names);
        let profile = self.active_profile.as_deref().unwrap_or("default");
        if let Some(status_hooks) = self.status_hook_configs.get(profile) {
            self.status_hook_config = status_hooks.clone();
        }
    }

    /// Toggle terminal mode between Container and Host for a session
    pub fn toggle_terminal_mode(&mut self, session_id: &str) {
        let current = self.get_terminal_mode(session_id);
        let new_mode = match current {
            TerminalMode::Container => TerminalMode::Host,
            TerminalMode::Host => TerminalMode::Container,
        };
        self.terminal_modes.insert(session_id.to_string(), new_mode);
    }

    pub fn start_container_terminal_for_instance_with_size(
        &mut self,
        id: &str,
        size: Option<(u16, u16)>,
    ) -> anyhow::Result<()> {
        self.try_mutate_instance(id, |inst| inst.start_container_terminal_with_size(size))
            .map(|_| ())
    }
}
