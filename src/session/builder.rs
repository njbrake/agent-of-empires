//! Instance creation and cleanup utilities.
//!
//! This module provides shared logic for building new session instances,
//! used by both synchronous (TUI operations) and asynchronous (background poller) code paths.

use std::path::PathBuf;

use anyhow::{bail, Result};
use chrono::Utc;

use crate::containers::{self, ContainerRuntimeInterface};
use crate::git::GitWorktree;

use super::{
    civilizations, Config, Instance, SandboxInfo, WorkspaceInfo, WorkspaceRepo, WorktreeInfo,
};

/// Parameters for creating a new session instance.
#[derive(Debug, Clone)]
pub struct InstanceParams {
    pub title: String,
    pub path: String,
    pub group: String,
    pub tool: String,
    pub worktree_enabled: bool,
    pub worktree_branch: Option<String>,
    pub create_new_branch: bool,
    pub sandbox: bool,
    /// The sandbox image to use. Required when sandbox is true.
    pub sandbox_image: String,
    pub yolo_mode: bool,
    /// Additional environment entries for the container.
    /// `KEY` = pass through from host, `KEY=VALUE` = set explicitly.
    pub extra_env: Vec<String>,
    /// Extra arguments to append after the agent binary
    pub extra_args: String,
    /// Command override for the agent binary (replaces the default binary)
    pub command_override: String,
    /// Additional repository paths for multi-repo workspace mode
    pub extra_repo_paths: Vec<String>,
}

/// Result of building an instance, tracking what was created for cleanup purposes.
pub struct BuildResult {
    pub instance: Instance,
    /// Path to worktree if one was created and managed by aoe
    pub created_worktree: Option<CreatedWorktree>,
    /// Workspace worktrees created during build (for cleanup)
    pub created_workspace_worktrees: Vec<CreatedWorktree>,
}

/// Info about a worktree created during instance building.
pub struct CreatedWorktree {
    pub path: PathBuf,
    pub main_repo_path: PathBuf,
}

/// Result of creating a multi-repo workspace.
pub struct WorkspaceResult {
    pub workspace_info: WorkspaceInfo,
    pub created_worktrees: Vec<CreatedWorktree>,
    pub workspace_path: PathBuf,
}

/// Create a multi-repo workspace with worktrees for each repository.
///
/// Validates repo paths, detects name collisions, creates worktrees inside
/// a shared workspace directory, and rolls back on any error.
pub fn create_workspace(
    primary_path: &std::path::Path,
    extra_repo_paths: &[PathBuf],
    branch: &str,
    create_new_branch: bool,
    workspace_template: &str,
) -> Result<WorkspaceResult> {
    let primary_main_repo = GitWorktree::find_main_repo(primary_path)?;
    let primary_git_wt = GitWorktree::new(primary_main_repo)?;

    let session_id = uuid::Uuid::new_v4().to_string();
    let session_id_short = &session_id[..8];

    let workspace_path =
        primary_git_wt.compute_path(branch, workspace_template, session_id_short)?;
    let workspace_dir = workspace_path.to_string_lossy().to_string();
    std::fs::create_dir_all(&workspace_path)?;

    let all_repo_paths: Vec<PathBuf> = std::iter::once(primary_path.to_path_buf())
        .chain(
            extra_repo_paths
                .iter()
                .map(|r| r.canonicalize().unwrap_or_else(|_| r.clone())),
        )
        .collect();

    // Check for duplicate repo directory names
    let mut seen_names = std::collections::HashSet::new();
    for repo_path in &all_repo_paths {
        let name = repo_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "repo".to_string());
        if !seen_names.insert(name.clone()) {
            let _ = std::fs::remove_dir_all(&workspace_path);
            bail!(
                "Duplicate repository name '{}' in workspace\n\
                 Tip: Rename one of the directories to avoid the collision",
                name
            );
        }
    }

    let mut repos = Vec::new();
    let mut created_worktrees: Vec<CreatedWorktree> = Vec::new();

    let cleanup = |created: &[CreatedWorktree], ws_path: &std::path::Path| {
        for wt in created {
            if let Ok(git_wt) = GitWorktree::new(wt.main_repo_path.clone()) {
                let _ = git_wt.remove_worktree(&wt.path, false);
            }
        }
        let _ = std::fs::remove_dir_all(ws_path);
    };

    for repo_path in &all_repo_paths {
        if !GitWorktree::is_git_repo(repo_path) {
            cleanup(&created_worktrees, &workspace_path);
            bail!(
                "Path is not in a git repository: {}\n\
                 Tip: All --repo paths must be git repositories",
                repo_path.display()
            );
        }

        let main_repo_path_raw = GitWorktree::find_main_repo(repo_path)?;
        let main_repo_path = main_repo_path_raw
            .canonicalize()
            .unwrap_or(main_repo_path_raw);
        let git_wt = GitWorktree::new(main_repo_path.clone())?;

        let repo_name = repo_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "repo".to_string());

        let worktree_subdir = workspace_path.join(&repo_name);

        if let Err(e) = git_wt.create_worktree(branch, &worktree_subdir, create_new_branch) {
            cleanup(&created_worktrees, &workspace_path);
            bail!("Failed to create worktree for {}: {}", repo_name, e);
        }

        created_worktrees.push(CreatedWorktree {
            path: worktree_subdir.clone(),
            main_repo_path: main_repo_path.clone(),
        });

        repos.push(WorkspaceRepo {
            name: repo_name,
            source_path: repo_path.to_string_lossy().to_string(),
            branch: branch.to_string(),
            worktree_path: worktree_subdir.to_string_lossy().to_string(),
            main_repo_path: main_repo_path.to_string_lossy().to_string(),
            managed_by_aoe: true,
        });
    }

    Ok(WorkspaceResult {
        workspace_info: WorkspaceInfo {
            branch: branch.to_string(),
            workspace_dir,
            repos,
            created_at: Utc::now(),
            cleanup_on_delete: true,
        },
        created_worktrees,
        workspace_path,
    })
}

/// Build an instance with all setup (worktree resolution, sandbox config).
///
/// This does NOT start the instance or create Docker containers - that happens
/// separately via `instance.start()`. This separation allows for proper cleanup
/// if starting fails.
pub fn build_instance(
    params: InstanceParams,
    existing_titles: &[&str],
    existing_branches: &[&str],
    profile: &str,
) -> Result<BuildResult> {
    // Host-only agents (e.g. settl) cannot run in a sandbox or use worktrees.
    let is_host_only = crate::agents::get_agent(&params.tool).is_some_and(|a| a.host_only);
    if is_host_only && params.sandbox {
        bail!(
            "{} can only run on the host, not in a sandbox.",
            params.tool
        );
    }
    if is_host_only && params.worktree_enabled {
        bail!("{} does not support worktree mode.", params.tool);
    }

    if params.sandbox {
        let runtime = containers::get_container_runtime();
        if !runtime.is_available() {
            bail!("Container runtime is not installed. Please install a supported runtime to use sandbox mode.");
        }
        if !runtime.is_daemon_running() {
            bail!("Container runtime daemon is not running. Please start a supported runtime to use sandbox mode.");
        }
    }

    let config =
        super::repo_config::resolve_config_with_repo(profile, std::path::Path::new(&params.path))
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to load config, using defaults: {}", e);
                Config::default()
            });

    let mut final_path = PathBuf::from(&params.path)
        .canonicalize()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| params.path.clone());

    let mut worktree_info = None;
    let mut created_worktree = None;
    let mut workspace_info = None;
    let mut created_workspace_worktrees: Vec<CreatedWorktree> = Vec::new();
    let final_title = resolve_title(
        &params.title,
        params.worktree_branch.as_deref(),
        params.worktree_enabled,
        existing_titles,
    );
    let branch_source = resolve_worktree_branch(
        params.worktree_enabled,
        params.worktree_branch.as_deref(),
        &final_title,
    );

    let effective_worktree_branch: Option<String> = match branch_source {
        None => None,
        Some(BranchSource::Explicit(name)) => Some(name),
        Some(BranchSource::Derived(name)) => {
            if params.create_new_branch {
                let mut taken: std::collections::HashSet<String> =
                    existing_branches.iter().map(|s| (*s).to_string()).collect();
                if let Ok(local) =
                    crate::git::diff::list_branches(std::path::Path::new(&params.path))
                {
                    taken.extend(local);
                }
                Some(dedupe_branch_name(&name, &taken))
            } else {
                Some(name)
            }
        }
    };

    if let Some(branch) = &effective_worktree_branch {
        if !params.extra_repo_paths.is_empty() {
            let primary_path = PathBuf::from(&params.path)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&params.path));
            let extra_paths: Vec<PathBuf> =
                params.extra_repo_paths.iter().map(PathBuf::from).collect();

            let ws_result = create_workspace(
                &primary_path,
                &extra_paths,
                branch,
                params.create_new_branch,
                &config.worktree.workspace_path_template,
            )?;

            final_path = ws_result.workspace_path.to_string_lossy().to_string();
            workspace_info = Some(ws_result.workspace_info);
            created_workspace_worktrees = ws_result.created_worktrees;
        } else {
            // Single worktree mode (existing logic)
            let path = PathBuf::from(&params.path);
            if !GitWorktree::is_git_repo(&path) {
                bail!("Path is not in a git repository");
            }
            let main_repo_path_raw = GitWorktree::find_main_repo(&path)?;
            let main_repo_path = main_repo_path_raw
                .canonicalize()
                .unwrap_or(main_repo_path_raw);
            let git_wt = GitWorktree::new(main_repo_path.clone())?;

            // Choose appropriate template based on repo type (bare vs regular)
            // Use main_repo_path (not path) to correctly detect bare repos when running from a worktree
            let is_bare = GitWorktree::is_bare_repo(&main_repo_path);
            let template = if is_bare {
                &config.worktree.bare_repo_path_template
            } else {
                &config.worktree.path_template
            };

            if !params.create_new_branch {
                let existing_worktrees = git_wt.list_worktrees()?;
                if let Some(existing) = existing_worktrees
                    .iter()
                    .find(|wt| wt.branch.as_deref() == Some(branch))
                {
                    final_path = existing.path.to_string_lossy().to_string();
                    worktree_info = Some(WorktreeInfo {
                        branch: branch.clone(),
                        main_repo_path: main_repo_path.to_string_lossy().to_string(),
                        managed_by_aoe: false,
                        created_at: Utc::now(),
                    });
                } else {
                    let session_id = uuid::Uuid::new_v4().to_string();
                    let worktree_path = git_wt.compute_path(branch, template, &session_id[..8])?;

                    git_wt.create_worktree(branch, &worktree_path, false)?;

                    final_path = worktree_path.to_string_lossy().to_string();
                    created_worktree = Some(CreatedWorktree {
                        path: worktree_path,
                        main_repo_path: main_repo_path.clone(),
                    });
                    worktree_info = Some(WorktreeInfo {
                        branch: branch.clone(),
                        main_repo_path: main_repo_path.to_string_lossy().to_string(),
                        managed_by_aoe: true,
                        created_at: Utc::now(),
                    });
                }
            } else {
                let session_id = uuid::Uuid::new_v4().to_string();
                let worktree_path = git_wt.compute_path(branch, template, &session_id[..8])?;

                if worktree_path.exists() {
                    bail!("Worktree already exists at {}", worktree_path.display());
                }

                git_wt.create_worktree(branch, &worktree_path, true)?;

                final_path = worktree_path.to_string_lossy().to_string();
                created_worktree = Some(CreatedWorktree {
                    path: worktree_path,
                    main_repo_path: main_repo_path.clone(),
                });
                worktree_info = Some(WorktreeInfo {
                    branch: branch.clone(),
                    main_repo_path: main_repo_path.to_string_lossy().to_string(),
                    managed_by_aoe: true,
                    created_at: Utc::now(),
                });
            }
        }
    }

    // Validate that the final path exists and is a directory.
    // This catches cases where the user typed a non-existent path in the TUI;
    // without this check tmux silently falls back to the home directory.
    let final_path_buf = PathBuf::from(&final_path);
    if !final_path_buf.exists() {
        bail!("Project path does not exist: {}", final_path);
    }
    if !final_path_buf.is_dir() {
        bail!("Project path is not a directory: {}", final_path);
    }

    let mut instance = Instance::new(&final_title, &final_path);
    instance.group_path = params.group;
    instance.tool = params.tool.clone();
    instance.detect_as = config
        .session
        .agent_detect_as
        .get(&params.tool)
        .cloned()
        .unwrap_or_default();
    instance.command = crate::agents::get_agent(&params.tool)
        .filter(|a| a.set_default_command)
        .map(|a| a.binary.to_string())
        .unwrap_or_default();
    instance.worktree_info = worktree_info;
    instance.workspace_info = workspace_info;
    instance.yolo_mode = params.yolo_mode;

    // Apply command overrides and custom agent commands from resolved config.
    // Priority: per-session params > agent_command_override > custom_agents > AgentDef default.
    if !params.command_override.is_empty() {
        instance.command = params.command_override;
    } else {
        let resolved = config.session.resolve_tool_command(&params.tool);
        if !resolved.is_empty() {
            instance.command = resolved;
        }
    }
    if !params.extra_args.is_empty() {
        instance.extra_args = params.extra_args;
    } else if let Some(extra) = config.session.agent_extra_args.get(&params.tool) {
        if !extra.is_empty() {
            instance.extra_args = extra.clone();
        }
    }

    if params.sandbox {
        instance.sandbox_info = Some(SandboxInfo {
            enabled: true,
            container_id: None,
            image: params.sandbox_image.clone(),
            container_name: containers::DockerContainer::generate_name(&instance.id),
            extra_env: if params.extra_env.is_empty() {
                None
            } else {
                Some(params.extra_env.clone())
            },
            custom_instruction: config.sandbox.custom_instruction.clone(),
        });
    }

    Ok(BuildResult {
        instance,
        created_worktree,
        created_workspace_worktrees,
    })
}

/// Clean up resources created during a failed or cancelled instance build.
///
/// This handles:
/// - Removing worktrees created by aoe
/// - Removing Docker containers
/// - Killing tmux sessions
pub fn cleanup_instance(
    instance: &Instance,
    created_worktree: Option<&CreatedWorktree>,
    created_workspace_worktrees: &[CreatedWorktree],
) {
    if let Some(wt) = created_worktree {
        if let Ok(git_wt) = GitWorktree::new(wt.main_repo_path.clone()) {
            if let Err(e) = git_wt.remove_worktree(&wt.path, false) {
                tracing::warn!("Failed to clean up worktree: {}", e);
            }
        }
    }

    // Workspace worktree cleanup
    for wt in created_workspace_worktrees {
        if let Ok(git_wt) = GitWorktree::new(wt.main_repo_path.clone()) {
            if let Err(e) = git_wt.remove_worktree(&wt.path, false) {
                tracing::warn!("Failed to clean up workspace worktree: {}", e);
            }
        }
    }
    // Clean up workspace directory if workspace was created
    if let Some(ws_info) = &instance.workspace_info {
        let _ = std::fs::remove_dir_all(&ws_info.workspace_dir);
    }

    if let Some(sandbox) = &instance.sandbox_info {
        if sandbox.enabled {
            let container = containers::DockerContainer::from_session_id(&instance.id);
            if container.exists().unwrap_or(false) {
                if let Err(e) = container.remove(true) {
                    tracing::warn!("Failed to clean up container: {}", e);
                }
            }
        }
    }

    let _ = instance.kill();
}

/// Resolve the session title: use the provided title, then an explicit worktree
/// branch name, then fall back to a random civilization name.
pub(crate) fn resolve_title(
    title: &str,
    worktree_branch: Option<&str>,
    worktree_enabled: bool,
    existing_titles: &[&str],
) -> String {
    if title.is_empty() {
        if worktree_enabled {
            if let Some(branch) = worktree_branch.filter(|b| !b.trim().is_empty()) {
                branch.trim().to_string()
            } else {
                civilizations::generate_random_title(existing_titles)
            }
        } else {
            civilizations::generate_random_title(existing_titles)
        }
    } else {
        title.to_string()
    }
}

/// Origin of an effective worktree branch name. The builder uses this to decide
/// whether collisions with existing branches should be resolved by suffixing
/// (Derived) or surfaced as an error (Explicit).
#[derive(Debug, Clone)]
pub(crate) enum BranchSource {
    /// User typed this name explicitly. Treat conflicts as a hard error.
    Explicit(String),
    /// Derived from the session title. Suffix on conflict.
    Derived(String),
}

fn resolve_worktree_branch(
    worktree_enabled: bool,
    worktree_branch: Option<&str>,
    final_title: &str,
) -> Option<BranchSource> {
    if !worktree_enabled {
        return None;
    }
    Some(
        match worktree_branch.map(str::trim).filter(|b| !b.is_empty()) {
            Some(b) => BranchSource::Explicit(b.to_string()),
            None => BranchSource::Derived(branch_name_from_title(final_title)),
        },
    )
}

/// Find the next branch name not present in `taken`.
/// If `base` is free, returns it unchanged. Otherwise appends `-2`, `-3`, …
/// until a free name is found.
fn dedupe_branch_name(base: &str, taken: &std::collections::HashSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    let mut n = 2usize;
    loop {
        let candidate = format!("{}-{}", base, n);
        if !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Map Latin ligatures and stroked letters to their conventional ASCII expansions.
/// NFKD decomposition handles accented characters (é → e + combining acute, then
/// the combining mark is dropped by the ASCII filter), but ligatures and stroked
/// letters have no canonical decomposition, so we expand them here.
fn expand_ligature(c: char) -> Option<&'static str> {
    Some(match c {
        'ß' => "ss",
        'æ' => "ae",
        'Æ' => "AE",
        'œ' => "oe",
        'Œ' => "OE",
        'ø' => "o",
        'Ø' => "O",
        'ł' => "l",
        'Ł' => "L",
        'đ' => "d",
        'Đ' => "D",
        'þ' => "th",
        'Þ' => "Th",
        _ => return None,
    })
}

pub(crate) fn branch_name_from_title(title: &str) -> String {
    use unicode_normalization::UnicodeNormalization;

    let mut branch = String::new();
    let mut last_was_dash = false;

    let mut push_processed = |ch: char| {
        let next = if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            Some(ch.to_ascii_lowercase())
        } else if ch.is_whitespace() || ch.is_ascii_punctuation() {
            Some('-')
        } else {
            None
        };

        if let Some(ch) = next {
            if ch == '-' {
                if branch.is_empty() || last_was_dash {
                    return;
                }
                last_was_dash = true;
            } else {
                last_was_dash = false;
            }
            branch.push(ch);
        }
    };

    for ch in title.trim().nfkd() {
        match expand_ligature(ch) {
            Some(expansion) => expansion.chars().for_each(&mut push_processed),
            None => push_processed(ch),
        }
    }

    while branch.ends_with('-') {
        branch.pop();
    }

    if branch.is_empty() {
        "session".to_string()
    } else {
        branch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_title_with_worktree_uses_branch_name() {
        let title = resolve_title("", Some("feature-auth"), true, &[]);
        assert_eq!(title, "feature-auth");
    }

    #[test]
    fn test_empty_title_without_worktree_uses_civilization() {
        let title = resolve_title("", None, false, &[]);
        assert!(
            civilizations::CIVILIZATIONS.contains(&title.as_str()),
            "Expected a civilization name, got: {}",
            title
        );
    }

    #[test]
    fn test_provided_title_with_worktree_keeps_title() {
        let title = resolve_title("My Session", Some("feature-auth"), true, &[]);
        assert_eq!(title, "My Session");
    }

    #[test]
    fn test_provided_title_without_worktree_keeps_title() {
        let title = resolve_title("Custom Name", None, false, &[]);
        assert_eq!(title, "Custom Name");
    }

    #[test]
    fn test_worktree_branch_derived_from_title_when_name_empty() {
        let branch = resolve_worktree_branch(true, None, "Fix Login Flow").unwrap();
        assert!(matches!(branch, BranchSource::Derived(ref s) if s == "fix-login-flow"));
    }

    #[test]
    fn test_worktree_branch_preserves_explicit_name() {
        let branch = resolve_worktree_branch(true, Some("feat/auth"), "Fix Login Flow").unwrap();
        assert!(matches!(branch, BranchSource::Explicit(ref s) if s == "feat/auth"));
    }

    #[test]
    fn test_worktree_branch_disabled_without_worktree() {
        assert!(resolve_worktree_branch(false, Some("feat/auth"), "Fix Login Flow").is_none());
    }

    #[test]
    fn test_branch_name_from_title_sanitizes_git_hostile_chars() {
        assert_eq!(
            branch_name_from_title("Fix: login @ mobile #42"),
            "fix-login-mobile-42"
        );
        assert_eq!(
            branch_name_from_title("feat/auth.refactor"),
            "feat-auth-refactor"
        );
    }

    #[test]
    fn test_branch_name_from_title_folds_latin_diacritics() {
        assert_eq!(branch_name_from_title("café fix"), "cafe-fix");
        assert_eq!(branch_name_from_title("naïve solution"), "naive-solution");
        assert_eq!(branch_name_from_title("Straße"), "strasse");
        assert_eq!(branch_name_from_title("Łódź"), "lodz");
        assert_eq!(branch_name_from_title("crème brûlée"), "creme-brulee");
        assert_eq!(branch_name_from_title("œuvre"), "oeuvre");
    }

    #[test]
    fn test_branch_name_from_title_drops_unsupported_scripts() {
        // CJK and emoji are not in the Latin transliteration table, so they're
        // stripped (current best-effort behavior). The "session" fallback kicks in
        // when nothing usable remains.
        assert_eq!(branch_name_from_title("测试"), "session");
        assert_eq!(branch_name_from_title("🚀 ship"), "ship");
    }

    #[test]
    fn test_dedupe_branch_name_returns_base_when_free() {
        let taken = std::collections::HashSet::new();
        assert_eq!(dedupe_branch_name("fix-bug", &taken), "fix-bug");
    }

    #[test]
    fn test_dedupe_branch_name_appends_suffix_on_collision() {
        let mut taken = std::collections::HashSet::new();
        taken.insert("fix-bug".to_string());
        assert_eq!(dedupe_branch_name("fix-bug", &taken), "fix-bug-2");

        taken.insert("fix-bug-2".to_string());
        taken.insert("fix-bug-3".to_string());
        assert_eq!(dedupe_branch_name("fix-bug", &taken), "fix-bug-4");
    }
}
