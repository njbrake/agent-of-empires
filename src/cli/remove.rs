//! `agent-of-empires remove` command implementation

use anyhow::{bail, Result};
use clap::Args;

use crate::session::{Instance, Storage};

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

#[tracing::instrument(target = "cli.session", skip_all, fields(profile = %profile))]
pub async fn run(profile: &str, args: RemoveArgs) -> Result<()> {
    let storage = Storage::new(profile)?;

    // This snapshot is read-only: it locates the target row so its worktree
    // and container teardown can run before any storage write. The
    // authoritative removal happens later via `storage.update`, which
    // re-loads under the cross-process lock; a wholesale `commit` of this
    // snapshot would clobber a row another process added during the
    // teardown (`perform_deletion` removes a worktree and a container, a
    // real staleness window).
    let instances = storage.load()?;

    let target: Option<Instance> = instances.into_iter().find(|inst| {
        inst.id == args.identifier
            || inst.id.starts_with(&args.identifier)
            || inst.title == args.identifier
    });

    let Some(inst) = target else {
        bail!(
            "Session not found in profile '{}': {}",
            storage.profile(),
            args.identifier
        );
    };

    let removed_title = inst.title.clone();
    let removed_id = inst.id.clone();

    let config = crate::session::repo_config::resolve_config_with_repo_or_warn(
        profile,
        std::path::Path::new(&inst.project_path),
    );

    let delete_worktree = needs_worktree_cleanup(&inst, &args);
    let delete_branch = inst
        .worktree_info
        .as_ref()
        .is_some_and(|wt| wt.managed_by_aoe)
        && (args.delete_branch || (delete_worktree && config.worktree.delete_branch_on_cleanup));
    let delete_sandbox = inst.sandbox_info.as_ref().is_some_and(|s| s.enabled)
        && !args.keep_container
        && config.sandbox.auto_cleanup;

    let result =
        crate::session::deletion::perform_deletion(&crate::session::deletion::DeletionRequest {
            session_id: inst.id.clone(),
            instance: inst.clone(),
            delete_worktree,
            delete_branch,
            delete_sandbox,
            force_delete: args.force,
            detach_hooks: false,
        });

    for msg in &result.messages {
        println!("  {}", msg);
    }
    for err in &result.errors {
        eprintln!("Warning: {}", err);
    }

    if !delete_worktree {
        if let Some(wt_info) = &inst.worktree_info {
            if wt_info.managed_by_aoe {
                println!(
                    "Worktree preserved at: {} (use --delete-worktree to remove)",
                    inst.project_path
                );
            }
        }
    }
    if let Some(sandbox) = &inst.sandbox_info {
        if sandbox.enabled {
            if args.keep_container {
                println!("Container preserved: {}", sandbox.container_name);
            } else if !config.sandbox.auto_cleanup {
                println!(
                    "Container preserved: {} (auto_cleanup disabled in config)",
                    sandbox.container_name
                );
            }
        }
    }

    // Remove the target row through `update`, which re-loads `sessions.json`
    // under the cross-process lock and retains by `Instance.id`, so any row
    // a concurrent writer added during the teardown above survives.
    storage.update(move |instances, _groups| {
        instances.retain(|inst| inst.id != removed_id);
        Ok(())
    })?;

    println!(
        "  Removed session: {} (from profile '{}')",
        removed_title,
        storage.profile()
    );

    Ok(())
}
