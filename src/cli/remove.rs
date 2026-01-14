//! `agent-of-empires remove` command implementation

use anyhow::{bail, Result};
use clap::Args;

use crate::docker::DockerContainer;
use crate::session::{GroupTree, Instance, Storage};

#[derive(Args)]
pub struct RemoveArgs {
    /// Session ID or title to remove
    identifier: String,

    /// Keep worktree directory (don't delete it)
    #[arg(short = 'k', long = "keep-worktree")]
    keep_worktree: bool,

    /// Keep Docker container (don't remove it)
    #[arg(long = "keep-container")]
    keep_container: bool,
}

fn needs_cleanup_confirmation(inst: &Instance, args: &RemoveArgs) -> (bool, bool) {
    let will_cleanup_worktree = inst
        .worktree_info
        .as_ref()
        .is_some_and(|wt| wt.managed_by_aoe && wt.cleanup_on_delete && !args.keep_worktree);
    let will_cleanup_container = inst
        .sandbox_info
        .as_ref()
        .is_some_and(|s| s.enabled && !args.keep_container);
    (will_cleanup_worktree, will_cleanup_container)
}

pub async fn run(profile: &str, args: RemoveArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (instances, groups) = storage.load_with_groups()?;

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

            let (will_cleanup_worktree, will_cleanup_container) =
                needs_cleanup_confirmation(&inst, &args);

            // Show combined warning and get confirmation
            let user_confirmed = if will_cleanup_worktree || will_cleanup_container {
                use std::io::{self, Write};

                println!("\nThis will delete:");
                if will_cleanup_worktree {
                    let wt_info = inst.worktree_info.as_ref().unwrap();
                    println!(
                        "  - Worktree: {} (branch: {})",
                        inst.project_path, wt_info.branch
                    );
                }
                if will_cleanup_container {
                    let sandbox = inst.sandbox_info.as_ref().unwrap();
                    println!("  - Docker container: {}", sandbox.container_name);
                }
                print!("\nProceed? (Y/n): ");
                io::stdout().flush()?;

                let mut response = String::new();
                io::stdin().read_line(&mut response)?;
                let response = response.trim().to_lowercase();

                response.is_empty() || response == "y" || response == "yes"
            } else {
                true
            };

            // Handle worktree cleanup
            if will_cleanup_worktree {
                if user_confirmed {
                    use crate::git::GitWorktree;
                    use std::path::PathBuf;

                    let wt_info = inst.worktree_info.as_ref().unwrap();
                    let worktree_path = PathBuf::from(&inst.project_path);
                    let main_repo = PathBuf::from(&wt_info.main_repo_path);

                    match GitWorktree::new(main_repo) {
                        Ok(git_wt) => {
                            if let Err(e) = git_wt.remove_worktree(&worktree_path) {
                                eprintln!("Warning: failed to remove worktree: {}", e);
                                eprintln!(
                                    "You may need to remove it manually with: git worktree remove {}",
                                    inst.project_path
                                );
                            } else {
                                println!("✓ Worktree removed");
                            }
                        }
                        Err(e) => {
                            eprintln!("Warning: failed to access git repository: {}", e);
                        }
                    }
                } else {
                    println!("Worktree preserved at: {}", inst.project_path);
                }
            }

            // Handle container cleanup
            if will_cleanup_container {
                if user_confirmed {
                    let sandbox = inst.sandbox_info.as_ref().unwrap();
                    let container = DockerContainer::from_session_id(&inst.id);

                    if container.exists().unwrap_or(false) {
                        match container.remove(true) {
                            Ok(_) => println!("✓ Container removed"),
                            Err(e) => {
                                eprintln!("Warning: failed to remove container: {}", e);
                                eprintln!(
                                    "   You can remove it manually with: docker rm -f {}",
                                    sandbox.container_name
                                );
                            }
                        }
                    }
                } else {
                    let sandbox = inst.sandbox_info.as_ref().unwrap();
                    println!("Container preserved: {}", sandbox.container_name);
                }
            } else if let Some(sandbox) = &inst.sandbox_info {
                if sandbox.enabled && args.keep_container {
                    println!("Container preserved: {}", sandbox.container_name);
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
        "✓ Removed session: {} (from profile '{}')",
        removed_title,
        storage.profile()
    );

    Ok(())
}
