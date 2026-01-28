//! `agent-of-empires add` command implementation

use anyhow::{bail, Result};
use clap::Args;
use std::path::PathBuf;

use crate::docker::{self, DockerContainer};
use crate::session::{civilizations, Config, GroupTree, Instance, SandboxInfo, Storage};

#[derive(Args)]
pub struct AddArgs {
    /// Project directory (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Session title (defaults to folder name)
    #[arg(short = 't', long)]
    title: Option<String>,

    /// Group path (defaults to parent folder)
    #[arg(short = 'g', long)]
    group: Option<String>,

    /// Command to run (e.g., 'claude', 'opencode', 'vibe', 'codex')
    #[arg(short = 'c', long = "cmd")]
    command: Option<String>,

    /// Parent session (creates sub-session, inherits group)
    #[arg(short = 'P', long)]
    parent: Option<String>,

    /// Launch the session immediately after creating
    #[arg(short = 'l', long)]
    launch: bool,

    /// Create session in a git worktree for the specified branch
    #[arg(short = 'w', long = "worktree")]
    worktree_branch: Option<String>,

    /// Create a new branch (use with --worktree)
    #[arg(short = 'b', long = "new-branch")]
    create_branch: bool,

    /// Run session in Docker sandbox
    #[arg(short = 's', long)]
    sandbox: bool,

    /// Custom Docker image for sandbox (implies --sandbox)
    #[arg(long = "sandbox-image")]
    sandbox_image: Option<String>,
}

pub async fn run(profile: &str, args: AddArgs) -> Result<()> {
    let mut path = if args.path.as_os_str() == "." {
        std::env::current_dir()?
    } else {
        args.path.canonicalize()?
    };

    if !path.is_dir() {
        bail!("Path is not a directory: {}", path.display());
    }

    let mut worktree_info_opt = None;

    if let Some(branch_raw) = &args.worktree_branch {
        use crate::git::GitWorktree;
        use crate::session::WorktreeInfo;
        use chrono::Utc;

        let branch = branch_raw.trim();

        if !GitWorktree::is_git_repo(&path) {
            bail!("Path is not in a git repository\nTip: Navigate to a git repository first");
        }

        let config = Config::load()?;

        let main_repo_path = GitWorktree::find_main_repo(&path)?;
        let git_wt = GitWorktree::new(main_repo_path.clone())?;

        let session_id = uuid::Uuid::new_v4().to_string();
        let session_id_short = &session_id[..8];

        // Choose appropriate template based on repo type (bare vs regular)
        // Use main_repo_path (not path) to correctly detect bare repos when running from a worktree
        let template = if GitWorktree::is_bare_repo(&main_repo_path) {
            &config.worktree.bare_repo_path_template
        } else {
            &config.worktree.path_template
        };
        let worktree_path = git_wt.compute_path(branch, template, session_id_short)?;

        if worktree_path.exists() {
            bail!(
                "Worktree already exists at {}\nTip: Use 'aoe add {}' to add the existing worktree",
                worktree_path.display(),
                worktree_path.display()
            );
        }

        println!("Creating worktree at: {}", worktree_path.display());
        git_wt.create_worktree(branch, &worktree_path, args.create_branch)?;

        path = worktree_path;

        worktree_info_opt = Some(WorktreeInfo {
            branch: branch.to_string(),
            main_repo_path: main_repo_path.to_string_lossy().to_string(),
            managed_by_aoe: true,
            created_at: Utc::now(),
            cleanup_on_delete: true,
        });

        println!("✓ Worktree created successfully");
    }

    let storage = Storage::new(profile)?;
    let (mut instances, groups) = storage.load_with_groups()?;

    // Resolve parent session if specified
    let mut group_path = args.group.clone();
    let parent_id = if let Some(parent_ref) = &args.parent {
        let parent = super::resolve_session(parent_ref, &instances)?;
        if parent.is_sub_session() {
            bail!("Cannot create sub-session of a sub-session (single level only)");
        }
        group_path = Some(parent.group_path.clone());
        Some(parent.id.clone())
    } else {
        None
    };

    // Generate title
    let final_title = if let Some(title) = &args.title {
        let trimmed_title = title.trim();
        if is_duplicate_session(&instances, trimmed_title, path.to_str().unwrap_or("")) {
            println!(
                "Session already exists with same title and path: {}",
                trimmed_title
            );
            return Ok(());
        }
        trimmed_title.to_string()
    } else {
        let existing_titles: Vec<&str> = instances.iter().map(|i| i.title.as_str()).collect();
        civilizations::generate_random_title(&existing_titles)
    };

    let mut instance = Instance::new(&final_title, path.to_str().unwrap_or(""));

    if let Some(group) = &group_path {
        instance.group_path = group.trim().to_string();
    }

    if let Some(parent) = parent_id {
        instance.parent_session_id = Some(parent);
    }

    if let Some(cmd) = &args.command {
        instance.command = cmd.clone();
        instance.tool = detect_tool(cmd)?;
    }

    if let Some(worktree_info) = worktree_info_opt {
        instance.worktree_info = Some(worktree_info);
    }

    // Handle sandbox setup
    let use_sandbox = args.sandbox || args.sandbox_image.is_some();
    let config = Config::load()?;

    if use_sandbox || config.sandbox.enabled_by_default {
        if !docker::is_docker_available() {
            if use_sandbox {
                bail!(
                    "Docker is not installed or not accessible.\n\
                     Install Docker: https://docs.docker.com/get-docker/\n\
                     Tip: Use 'aoe add' without --sandbox to run directly on host"
                );
            }
        } else {
            let container_name = DockerContainer::generate_name(&instance.id);
            let image = args
                .sandbox_image
                .as_ref()
                .map(|s| s.trim().to_string())
                .unwrap_or_else(docker::effective_default_image);
            instance.sandbox_info = Some(SandboxInfo {
                enabled: true,
                container_id: None,
                image,
                container_name,
                created_at: None,
                yolo_mode: None,
                extra_env_keys: None,
            });
        }
    }

    instances.push(instance.clone());

    // Rebuild group tree
    let mut group_tree = GroupTree::new_with_groups(&instances, &groups);
    if !instance.group_path.is_empty() {
        group_tree.create_group(&instance.group_path);
    }

    storage.save_with_groups(&instances, &group_tree)?;

    println!("✓ Added session: {}", final_title);
    println!("  Profile: {}", storage.profile());
    println!("  Path:    {}", path.display());
    println!("  Group:   {}", instance.group_path);
    println!("  ID:      {}", instance.id);
    if let Some(cmd) = &args.command {
        println!("  Cmd:     {}", cmd);
    }
    if let Some(parent) = &args.parent {
        println!("  Parent:  {}", parent);
    }
    if instance.sandbox_info.is_some() {
        println!("  Sandbox: enabled");
    }

    if args.launch {
        let idx = instances
            .iter()
            .position(|i| i.id == instance.id)
            .expect("just added instance");
        instances[idx].start_with_size(crate::terminal::get_size())?;
        storage.save_with_groups(&instances, &group_tree)?;

        let tmux_session = crate::tmux::Session::new(&instance.id, &instance.title)?;
        tmux_session.attach()?;
    } else {
        println!();
        println!("Next steps:");
        println!(
            "  agent-of-empires session start {}   # Start the session",
            final_title
        );
        println!("  agent-of-empires                         # Open TUI and press Enter to attach");
    }

    Ok(())
}

pub fn is_duplicate_session(instances: &[Instance], title: &str, path: &str) -> bool {
    let normalized_path = path.trim_end_matches('/');
    instances.iter().any(|inst| {
        let existing_path = inst.project_path.trim_end_matches('/');
        existing_path == normalized_path && inst.title == title
    })
}

pub fn generate_unique_title(instances: &[Instance], base_title: &str, path: &str) -> String {
    let title_exists = |title: &str| -> bool {
        instances
            .iter()
            .any(|inst| inst.project_path == path && inst.title == title)
    };

    if !title_exists(base_title) {
        return base_title.to_string();
    }

    for i in 2..=100 {
        let candidate = format!("{} ({})", base_title, i);
        if !title_exists(&candidate) {
            return candidate;
        }
    }

    format!("{} ({})", base_title, chrono::Utc::now().timestamp())
}

fn detect_tool(cmd: &str) -> Result<String> {
    let cmd_lower = cmd.to_lowercase();
    if cmd_lower.is_empty() || cmd_lower.contains("claude") {
        Ok("claude".to_string())
    } else if cmd_lower.contains("opencode") || cmd_lower.contains("open-code") {
        Ok("opencode".to_string())
    } else if cmd_lower.contains("vibe") || cmd_lower.contains("mistral-vibe") {
        Ok("vibe".to_string())
    } else if cmd_lower.contains("codex") {
        Ok("codex".to_string())
    } else {
        bail!(
            "Unknown tool in command: {}\n\
             Supported tools: claude, opencode, vibe, codex\n\
             Tip: Command must contain one of the supported tool names",
            cmd
        )
    }
}
