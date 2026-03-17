//! `agent-of-empires remove` command implementation

use anyhow::{bail, Result};
use clap::Args;

use crate::containers;
use crate::git::cleanup::{cleanup_sandbox_worktree, is_permission_error, remove_worktree_dir};
use crate::git::GitWorktree;
use crate::session::{GroupTree, Instance, Storage};
use std::path::PathBuf;

#[derive(Args)]
pub struct RemoveArgs {
    /// Session ID or title to remove
    identifier: String,

    /// Delete worktree directory (default: keep worktree)
    #[arg(long = "delete-worktree")]
    delete_worktree: bool,

    /// Delete git branch after worktree removal (default: per config)
    #[arg(long = "delete-branch")]
    delete_branch: bool,

    /// Force worktree removal even with untracked/modified files
    #[arg(long)]
    force: bool,

    /// Keep container instead of deleting it (default: delete per config)
    #[arg(long = "keep-container")]
    keep_container: bool,
}

fn needs_worktree_cleanup(inst: &Instance, args: &RemoveArgs) -> bool {
    inst.worktree_info
        .as_ref()
        .is_some_and(|wt| wt.managed_by_aoe && args.delete_worktree)
}

pub async fn run(profile: &str, args: RemoveArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (instances, groups) = storage.load_with_groups()?;
    let config = crate::session::resolve_config(profile).unwrap_or_default();

    let mut found = false;
    let mut removed_title = String::new();
    let mut new_instances = Vec::with_capacity(instances.len());

    for inst in instances {
        if inst.id == args.identifier
            || inst.id.starts_with(&args.identifier)
            || inst.title == args.identifier
        {
            found = true;
            removed_title = inst.title.clone();

            let will_cleanup_worktree = needs_worktree_cleanup(&inst, &args);
            // Delete branch if explicitly requested, or if worktree is being
            // deleted and config says to also delete the branch.
            let will_delete_branch = inst
                .worktree_info
                .as_ref()
                .is_some_and(|wt| wt.managed_by_aoe)
                && (args.delete_branch
                    || (will_cleanup_worktree && config.worktree.delete_branch_on_cleanup));

            // Track whether worktree removal succeeded (needed for branch deletion)
            let mut worktree_removed = false;

            // Handle worktree cleanup
            if will_cleanup_worktree {
                let wt_info = inst.worktree_info.as_ref().unwrap();
                let worktree_path = PathBuf::from(&inst.project_path);
                let main_repo = PathBuf::from(&wt_info.main_repo_path);

                match GitWorktree::new(main_repo.clone()) {
                    Ok(git_wt) => {
                        if !worktree_path.join(".git").exists() {
                            // .git is missing. Remove the dir and prune.
                            if let Err(e) =
                                remove_worktree_dir(&worktree_path, &main_repo, args.force)
                            {
                                if inst.is_sandboxed()
                                    && is_permission_error(&e.to_string())
                                    && cleanup_sandbox_worktree(&inst)
                                {
                                    let container =
                                        containers::DockerContainer::from_session_id(&inst.id);
                                    let _ = container.remove(true);
                                    if let Err(e2) =
                                        remove_worktree_dir(&worktree_path, &main_repo, true)
                                    {
                                        eprintln!("Warning: failed to remove worktree dir: {}", e2);
                                    }
                                } else {
                                    eprintln!("Warning: failed to remove worktree dir: {}", e);
                                }
                            }
                            if let Err(e) = git_wt.prune_worktrees() {
                                eprintln!("Warning: failed to prune worktrees: {}", e);
                            } else {
                                worktree_removed = true;
                                println!("  Worktree removed");
                            }
                        } else {
                            let result = git_wt.remove_worktree(&worktree_path, args.force);
                            match result {
                                Ok(()) => {
                                    worktree_removed = true;
                                    println!("  Worktree removed");
                                }
                                Err(e) => {
                                    let err_str = e.to_string();
                                    if inst.is_sandboxed()
                                        && is_permission_error(&err_str)
                                        && cleanup_sandbox_worktree(&inst)
                                    {
                                        // Container cleanup deletes everything
                                        // including .git, so git worktree remove
                                        // won't work. Force-remove the container to
                                        // release the bind mount, then remove the
                                        // dir and prune.
                                        let container =
                                            containers::DockerContainer::from_session_id(&inst.id);
                                        let _ = container.remove(true);
                                        if let Err(e2) =
                                            remove_worktree_dir(&worktree_path, &main_repo, true)
                                        {
                                            eprintln!(
                                                "Warning: failed to remove worktree dir: {}",
                                                e2
                                            );
                                        }
                                        if let Err(e2) = git_wt.prune_worktrees() {
                                            eprintln!("Warning: failed to prune worktrees: {}", e2);
                                        } else {
                                            worktree_removed = true;
                                            println!("  Worktree removed");
                                        }
                                    } else {
                                        eprintln!("Warning: failed to remove worktree: {}", e);
                                        eprintln!(
                                                "You may need to remove it manually with: git worktree remove {}",
                                                inst.project_path
                                            );
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: failed to access git repository: {}", e);
                    }
                }
            } else if let Some(wt_info) = &inst.worktree_info {
                if wt_info.managed_by_aoe {
                    println!(
                        "Worktree preserved at: {} (use --delete-worktree to remove)",
                        inst.project_path
                    );
                }
            }

            // Handle branch cleanup (only if worktree was removed or wasn't requested)
            if will_delete_branch {
                let worktree_ok = !will_cleanup_worktree || worktree_removed;
                if worktree_ok {
                    let wt_info = inst.worktree_info.as_ref().unwrap();
                    let main_repo = PathBuf::from(&wt_info.main_repo_path);
                    match GitWorktree::new(main_repo) {
                        Ok(git_wt) => {
                            if let Err(e) = git_wt.delete_branch(&wt_info.branch) {
                                eprintln!("Warning: failed to delete branch: {}", e);
                            } else {
                                println!("  Branch '{}' deleted", wt_info.branch);
                            }
                        }
                        Err(e) => {
                            eprintln!("Warning: failed to access git repository: {}", e);
                        }
                    }
                }
            }

            // Kill tmux session if it exists
            if let Ok(tmux_session) = crate::tmux::Session::new(&inst.id, &inst.title) {
                if tmux_session.exists() {
                    if let Err(e) = tmux_session.kill() {
                        eprintln!("Warning: failed to kill tmux session: {}", e);
                        eprintln!(
                            "Session removed from Agent of Empires but may still be running in tmux"
                        );
                    }
                }
            }

            // Container cleanup (if config allows and user didn't request --keep-container)
            if let Some(sandbox) = &inst.sandbox_info {
                if sandbox.enabled && !args.keep_container {
                    if config.sandbox.auto_cleanup {
                        let container = containers::DockerContainer::from_session_id(&inst.id);
                        if container.exists().unwrap_or(false) {
                            if let Err(e) = container.remove(true) {
                                eprintln!("Warning: failed to remove container: {}", e);
                            } else {
                                println!("  Container removed");
                            }
                        }
                    } else {
                        println!(
                            "Container preserved: {} (auto_cleanup disabled in config)",
                            sandbox.container_name
                        );
                    }
                } else if args.keep_container {
                    println!("Container preserved: {}", sandbox.container_name);
                }
            }
        } else {
            new_instances.push(inst);
        }
    }

    if !found {
        bail!(
            "Session not found in profile '{}': {}",
            storage.profile(),
            args.identifier
        );
    }

    // Rebuild group tree and save
    let group_tree = GroupTree::new_with_groups(&new_instances, &groups);
    storage.save_with_groups(&new_instances, &group_tree)?;

    println!(
        "  Removed session: {} (from profile '{}')",
        removed_title,
        storage.profile()
    );

    Ok(())
}
