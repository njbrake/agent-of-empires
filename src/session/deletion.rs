//! Shared session deletion logic used by both TUI and web server.

use std::path::{Path, PathBuf};

use crate::containers::DockerContainer;
use crate::git::cleanup::remove_managed_worktree;
use crate::git::GitWorktree;
use crate::session::repo_config;
use crate::session::Instance;

pub struct DeletionRequest {
    pub session_id: String,
    pub instance: Instance,
    pub delete_worktree: bool,
    pub delete_branch: bool,
    pub delete_sandbox: bool,
    pub force_delete: bool,
}

#[derive(Debug)]
pub struct DeletionResult {
    pub session_id: String,
    pub success: bool,
    pub error: Option<String>,
}

pub fn perform_deletion(request: &DeletionRequest) -> DeletionResult {
    let mut errors = Vec::new();

    // Execute on_destroy hooks before any cleanup so resources (containers,
    // worktrees) are still available for teardown commands.
    run_on_destroy_hooks(&request.instance);

    // Track branch info for potential deletion after worktree removal
    let branch_to_delete = if request.delete_branch {
        request
            .instance
            .worktree_info
            .as_ref()
            .filter(|wt| wt.managed_by_aoe)
            .map(|wt| (wt.branch.clone(), PathBuf::from(&wt.main_repo_path)))
    } else {
        None
    };

    // Worktree cleanup (if user opted to delete it)
    // Must happen before branch deletion since the worktree is using the branch
    if request.delete_worktree {
        if let Some(wt_info) = &request.instance.worktree_info {
            if wt_info.managed_by_aoe {
                let worktree_path = PathBuf::from(&request.instance.project_path);
                let main_repo = PathBuf::from(&wt_info.main_repo_path);

                match GitWorktree::new(main_repo.clone()) {
                    Ok(git_wt) => {
                        if let Err(errs) = remove_managed_worktree(
                            &git_wt,
                            &worktree_path,
                            &main_repo,
                            &request.instance,
                            request.force_delete,
                        ) {
                            errors.extend(errs);
                        }
                    }
                    Err(e) => {
                        errors.push(format!("Worktree: {}", e));
                    }
                }
            }
        }
    }

    // Workspace cleanup (if user opted to delete worktrees and instance has workspace_info)
    if request.delete_worktree {
        if let Some(ws_info) = &request.instance.workspace_info {
            if ws_info.cleanup_on_delete {
                for repo in &ws_info.repos {
                    if repo.managed_by_aoe {
                        let worktree_path = PathBuf::from(&repo.worktree_path);
                        let main_repo = PathBuf::from(&repo.main_repo_path);

                        match GitWorktree::new(main_repo.clone()) {
                            Ok(git_wt) => {
                                if let Err(errs) = remove_managed_worktree(
                                    &git_wt,
                                    &worktree_path,
                                    &main_repo,
                                    &request.instance,
                                    request.force_delete,
                                ) {
                                    errors.extend(
                                        errs.into_iter()
                                            .map(|e| format!("Workspace ({}): {}", repo.name, e)),
                                    );
                                }
                            }
                            Err(e) => {
                                errors.push(format!("Workspace ({}): {}", repo.name, e));
                            }
                        }
                    }
                }
                // Remove workspace parent directory
                let ws_path = PathBuf::from(&ws_info.workspace_dir);
                if ws_path.exists() {
                    if let Err(e) = std::fs::remove_dir_all(&ws_path) {
                        errors.push(format!("Workspace dir: {}", e));
                    }
                }
            }
        }
    }

    // Branch cleanup (if user opted to delete it and worktree was successfully removed)
    if let Some((branch, main_repo)) = branch_to_delete {
        let worktree_ok =
            !request.delete_worktree || !errors.iter().any(|e| e.starts_with("Worktree:"));
        if worktree_ok {
            match GitWorktree::new(main_repo) {
                Ok(git_wt) => {
                    if let Err(e) = git_wt.delete_branch(&branch) {
                        errors.push(format!("Branch: {}", e));
                    }
                }
                Err(e) => {
                    errors.push(format!("Branch: {}", e));
                }
            }
        }
    }

    // Branch cleanup for workspace repos
    if request.delete_branch {
        if let Some(ws_info) = &request.instance.workspace_info {
            let worktree_ok =
                !request.delete_worktree || !errors.iter().any(|e| e.starts_with("Workspace ("));
            if worktree_ok {
                for repo in &ws_info.repos {
                    if repo.managed_by_aoe {
                        let main_repo = PathBuf::from(&repo.main_repo_path);
                        if let Ok(git_wt) = GitWorktree::new(main_repo) {
                            if let Err(e) = git_wt.delete_branch(&repo.branch) {
                                errors.push(format!("Branch ({}): {}", repo.name, e));
                            }
                        }
                    }
                }
            }
        }
    }

    // Container cleanup (if user opted to delete it)
    if request.delete_sandbox {
        if let Some(sandbox) = &request.instance.sandbox_info {
            if sandbox.enabled {
                let container = DockerContainer::from_session_id(&request.instance.id);
                if container.exists().unwrap_or(false) {
                    if let Err(e) = container.remove(true) {
                        errors.push(format!("Container: {}", e));
                    }
                }
            }
        }
    }

    // Tmux kill - non-fatal if session already gone
    let _ = request.instance.kill();

    // Kill paired terminal session if it exists
    let _ = request.instance.kill_terminal();

    // Clean up hook status files
    crate::hooks::cleanup_hook_status_dir(&request.instance.id);

    DeletionResult {
        session_id: request.session_id.clone(),
        success: errors.is_empty(),
        error: if errors.is_empty() {
            None
        } else {
            Some(errors.join("; "))
        },
    }
}

/// Run on_destroy hooks for an instance. Uses best-effort execution so all
/// hooks are attempted even if some fail. Failures are logged as warnings
/// and never prevent deletion.
///
/// Global/profile hooks are implicitly trusted. Repo-level hooks go through
/// the same trust verification as on_launch: if the hooks hash has changed
/// since the user last approved, repo hooks are silently skipped.
fn run_on_destroy_hooks(instance: &Instance) {
    let profile = if instance.source_profile.is_empty() {
        "default"
    } else {
        &instance.source_profile
    };

    let project_path = Path::new(&instance.project_path);

    // Start with global+profile on_destroy hooks (implicitly trusted).
    let mut resolved_on_destroy = crate::session::profile_config::resolve_config(profile)
        .map(|c| c.hooks.on_destroy)
        .unwrap_or_default();

    // Check if repo has trusted hooks that override.
    match repo_config::check_hook_trust(project_path) {
        Ok(repo_config::HookTrustStatus::Trusted(hooks)) if !hooks.on_destroy.is_empty() => {
            resolved_on_destroy = hooks.on_destroy.clone();
        }
        Ok(repo_config::HookTrustStatus::NeedsTrust { .. }) => {
            tracing::warn!(
                "Repo hooks changed since last trust approval; skipping repo on_destroy hooks"
            );
        }
        _ => {}
    }

    if resolved_on_destroy.is_empty() {
        return;
    }

    tracing::info!("Running on_destroy hooks for session {}", instance.id);

    let is_sandboxed = instance.sandbox_info.as_ref().is_some_and(|s| s.enabled);

    let errors = if is_sandboxed {
        if let Some(ref sandbox) = instance.sandbox_info {
            let workdir = instance.container_workdir();
            repo_config::execute_hooks_in_container_best_effort(
                &resolved_on_destroy,
                &sandbox.container_name,
                &workdir,
            )
        } else {
            vec![]
        }
    } else {
        repo_config::execute_hooks_best_effort(&resolved_on_destroy, project_path)
    };

    if !errors.is_empty() {
        tracing::warn!(
            "on_destroy hooks had {} failure(s) for session {}",
            errors.len(),
            instance.id
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_instance() -> Instance {
        Instance::new("Test Session", "/tmp/test-project")
    }

    #[test]
    fn test_deletion_result_success_when_no_worktree_or_sandbox() {
        let instance = create_test_instance();
        let request = DeletionRequest {
            session_id: instance.id.clone(),
            instance,
            delete_worktree: false,
            delete_branch: false,
            delete_sandbox: false,
            force_delete: false,
        };

        let result = perform_deletion(&request);

        assert!(result.success);
        assert!(result.error.is_none());
        assert_eq!(result.session_id, request.session_id);
    }

    #[test]
    fn test_deletion_result_success_even_with_delete_worktree_flag_when_no_worktree() {
        let instance = create_test_instance();
        let request = DeletionRequest {
            session_id: instance.id.clone(),
            instance,
            delete_worktree: true,
            delete_branch: false,
            delete_sandbox: false,
            force_delete: false,
        };

        let result = perform_deletion(&request);

        assert!(result.success);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_deletion_request_preserves_session_id() {
        let instance = create_test_instance();
        let custom_id = "custom-session-id-123".to_string();

        let request = DeletionRequest {
            session_id: custom_id.clone(),
            instance,
            delete_worktree: false,
            delete_branch: false,
            delete_sandbox: false,
            force_delete: false,
        };

        let result = perform_deletion(&request);
        assert_eq!(result.session_id, custom_id);
    }
}
