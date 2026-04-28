//! `agent-of-empires add` command implementation

use anyhow::{bail, Result};
use clap::Args;
use std::path::PathBuf;

use crate::containers::{self, ContainerRuntimeInterface};
use crate::session::builder;
use crate::session::repo_config;
use crate::session::{civilizations, GroupTree, Instance, SandboxInfo, Storage};

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

    /// Command to run (e.g., 'claude' or any other supported agent)
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

    /// Additional repositories for multi-repo workspace (use with --worktree)
    #[arg(long = "repo", short = 'r')]
    extra_repos: Vec<PathBuf>,

    /// Run session in Docker sandbox
    #[arg(short = 's', long)]
    sandbox: bool,

    /// Custom Docker image for sandbox (implies --sandbox)
    #[arg(long = "sandbox-image")]
    sandbox_image: Option<String>,

    /// Enable YOLO mode (skip permission prompts)
    #[arg(short = 'y', long)]
    yolo: bool,

    /// Automatically trust repository hooks without prompting
    #[arg(long = "trust-hooks")]
    trust_hooks: bool,

    /// Extra arguments to append after the agent binary
    #[arg(long)]
    extra_args: Option<String>,

    /// Override the agent binary command
    #[arg(long)]
    cmd_override: Option<String>,
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

    if !args.extra_repos.is_empty() && args.worktree_branch.is_none() {
        bail!("--repo requires --worktree to specify a branch\nTip: aoe add /path --repo /other -w branch-name");
    }

    let config = repo_config::resolve_config_with_repo(profile, &path).unwrap_or_default();

    // Preserve the original project path for hook trust checking.
    // `path` gets reassigned to the worktree/workspace directory below,
    // but hooks are defined in the original repo's `.agent-of-empires/config.toml`.
    let original_project_path = path.clone();

    let mut worktree_info_opt = None;
    let mut workspace_info_opt = None;

    if let Some(branch_raw) = &args.worktree_branch {
        use crate::git::GitWorktree;
        use crate::session::WorktreeInfo;
        use chrono::Utc;

        let branch = branch_raw.trim();

        if !args.extra_repos.is_empty() {
            let ws_result = builder::create_workspace(
                &path,
                &args.extra_repos,
                branch,
                args.create_branch,
                &config.worktree.workspace_path_template,
            )?;

            for repo in &ws_result.workspace_info.repos {
                println!(
                    "  Created worktree: {} -> {}",
                    repo.name, repo.worktree_path
                );
            }

            path = ws_result.workspace_path;
            workspace_info_opt = Some(ws_result.workspace_info);

            println!("✓ Workspace created successfully");
        } else {
            // Single worktree mode (existing logic)
            if !GitWorktree::is_git_repo(&path) {
                bail!("Path is not in a git repository\nTip: Navigate to a git repository first");
            }

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
            });

            println!("✓ Worktree created successfully");
        }
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

    // Generate title: use provided title, or branch name for worktree sessions, or random civ
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
    } else if let Some(ref branch) = args.worktree_branch {
        let branch_title = branch.trim().to_string();
        if is_duplicate_session(&instances, &branch_title, path.to_str().unwrap_or("")) {
            println!(
                "Session already exists with same title and path: {}",
                branch_title
            );
            return Ok(());
        }
        branch_title
    } else {
        let existing_titles: Vec<&str> = instances.iter().map(|i| i.title.as_str()).collect();
        civilizations::generate_random_title(&existing_titles)
    };

    let mut instance = Instance::new(&final_title, path.to_str().unwrap_or(""));
    instance.source_profile = profile.to_string();

    if let Some(group) = &group_path {
        instance.group_path = group.trim().to_string();
    }

    if let Some(parent) = parent_id {
        instance.parent_session_id = Some(parent);
    }

    if let Some(cmd) = &args.command {
        let tool_name = detect_tool(cmd)?;
        // Verify the agent binary is actually on PATH before creating the session
        if let Some(agent_def) = crate::agents::get_agent(&tool_name) {
            if !crate::tmux::is_agent_available(agent_def) {
                bail!(
                    "'{}' is not installed or not on $PATH.\n\
                     Install with: {}\n\
                     See all supported agents: aoe agents",
                    agent_def.binary,
                    agent_def.install_hint
                );
            }
        }
        instance.tool = tool_name;
        // Only store a custom command when the user passed extra args
        // (e.g. "claude --resume xyz"). A bare tool name/alias should resolve
        // through the agent definition so the correct binary is used.
        if cmd.trim().contains(' ') {
            instance.command = cmd.clone();
        }
    } else {
        // Use default_tool from resolved config, then first available tool, then "claude".
        // Check custom_agents first (exact match) before resolve_tool_name (substring match),
        // so names like "lenovo-claude" resolve as the custom agent, not built-in "claude".
        let available_tools = crate::tmux::AvailableTools::detect();
        let tools_list = available_tools.available_list();
        instance.tool = config
            .session
            .default_tool
            .as_deref()
            .and_then(|name| {
                if config.session.custom_agents.contains_key(name) {
                    Some(name)
                } else {
                    crate::agents::resolve_tool_name(name)
                }
            })
            .or_else(|| tools_list.first().map(|s| s.as_str()))
            .unwrap_or("claude")
            .to_string();
    }

    // Set detect_as for status detection (resolved once, avoids config load in poll loop)
    instance.detect_as = config
        .session
        .agent_detect_as
        .get(&instance.tool)
        .cloned()
        .unwrap_or_default();

    // Apply set_default_command for agents that need it (e.g., opencode, codex)
    if instance.command.is_empty() {
        instance.command = crate::agents::get_agent(&instance.tool)
            .filter(|a| a.set_default_command)
            .map(|a| a.binary.to_string())
            .unwrap_or_default();
    }

    if let Some(worktree_info) = worktree_info_opt {
        instance.worktree_info = Some(worktree_info);
    }

    if let Some(workspace_info) = workspace_info_opt {
        instance.workspace_info = Some(workspace_info);
    }

    instance.yolo_mode = args.yolo || config.session.yolo_mode_default;

    // Apply extra_args and command override: CLI flags take priority, then config defaults
    if let Some(ref extra) = args.extra_args {
        instance.extra_args = extra.clone();
    } else if let Some(extra) = config.session.agent_extra_args.get(&instance.tool) {
        if !extra.is_empty() {
            instance.extra_args = extra.clone();
        }
    }

    if let Some(ref cmd) = args.cmd_override {
        instance.command = cmd.clone();
    } else {
        let resolved = config.session.resolve_tool_command(&instance.tool);
        if !resolved.is_empty() {
            instance.command = resolved;
        }
    }

    // Handle sandbox setup
    let use_sandbox = args.sandbox || args.sandbox_image.is_some();

    let runtime = containers::get_container_runtime();
    if use_sandbox || config.sandbox.enabled_by_default {
        if !runtime.is_available() {
            if use_sandbox {
                bail!(
                    "Container runtime is not installed or not accessible.\n\
                     Install Docker: https://docs.docker.com/get-docker/\n\
                     Or on macOS: Apple Container\n\
                     Tip: Use 'aoe add' without --sandbox to run directly on host"
                );
            }
        } else {
            let container_name = containers::DockerContainer::generate_name(&instance.id);
            let image = args
                .sandbox_image
                .as_ref()
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| runtime.effective_default_image());
            instance.sandbox_info = Some(SandboxInfo {
                enabled: true,
                container_id: None,
                image,
                container_name,
                extra_env: None,
                custom_instruction: config.sandbox.custom_instruction.clone(),
            });
        }
    }

    // Check for repository hooks.
    // Use the original project path for trust checking (not the worktree/workspace
    // path, which won't contain `.agent-of-empires/config.toml`).
    let hook_result: Result<()> = (|| {
        let resolved_hooks: Option<crate::session::HooksConfig> =
            match repo_config::check_hook_trust(&original_project_path) {
                Ok(repo_config::HookTrustStatus::NeedsTrust { hooks, hooks_hash }) => {
                    let should_trust = if args.trust_hooks {
                        true
                    } else {
                        println!("\nRepository hooks detected in .agent-of-empires/config.toml:");
                        if !hooks.on_create.is_empty() {
                            println!("  on_create:");
                            for cmd in &hooks.on_create {
                                println!("    {}", cmd);
                            }
                        }
                        if !hooks.on_launch.is_empty() {
                            println!("  on_launch:");
                            for cmd in &hooks.on_launch {
                                println!("    {}", cmd);
                            }
                        }
                        print!("\nTrust and run these hooks? [y/N] ");
                        use std::io::Write;
                        std::io::stdout().flush()?;
                        let mut input = String::new();
                        std::io::stdin().read_line(&mut input)?;
                        input.trim().eq_ignore_ascii_case("y")
                    };

                    if should_trust {
                        repo_config::trust_repo(&original_project_path, &hooks_hash)?;
                        println!("✓ Repository hooks trusted");
                        repo_config::merge_hooks_with_config(profile, hooks)
                    } else {
                        println!("Hooks skipped (session created without running hooks)");
                        None
                    }
                }
                Ok(repo_config::HookTrustStatus::Trusted(repo_hooks)) => {
                    repo_config::merge_hooks_with_config(profile, repo_hooks)
                }
                Ok(repo_config::HookTrustStatus::NoHooks) => {
                    repo_config::resolve_global_profile_hooks(profile)
                }
                Err(e) => {
                    tracing::warn!("Failed to check repo hooks: {}", e);
                    repo_config::resolve_global_profile_hooks(profile)
                }
            };

        if let Some(hooks) = resolved_hooks {
            if !hooks.on_create.is_empty() {
                println!("Running on_create hooks...");
                repo_config::execute_hooks(&hooks.on_create, &path)?;
                println!("✓ on_create hooks completed");
            }
        }
        Ok(())
    })();

    if let Err(e) = hook_result {
        // Clean up worktree if we created one
        if let Some(ref wt_info) = instance.worktree_info {
            if wt_info.managed_by_aoe {
                if let Ok(git_wt) =
                    crate::git::GitWorktree::new(std::path::PathBuf::from(&wt_info.main_repo_path))
                {
                    let _ = git_wt.remove_worktree(&path, false);
                }
            }
        }
        // Clean up workspace worktrees if we created them
        if let Some(ref ws_info) = instance.workspace_info {
            for repo in &ws_info.repos {
                if repo.managed_by_aoe {
                    let wt_path = PathBuf::from(&repo.worktree_path);
                    let main_repo = PathBuf::from(&repo.main_repo_path);
                    if let Ok(git_wt) = crate::git::GitWorktree::new(main_repo) {
                        let _ = git_wt.remove_worktree(&wt_path, false);
                    }
                }
            }
            let _ = std::fs::remove_dir_all(&ws_info.workspace_dir);
        }
        return Err(e);
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
    if instance.yolo_mode {
        println!("  YOLO:    enabled");
    }
    if let Some(ws) = &instance.workspace_info {
        println!("  Workspace: {} repos", ws.repos.len());
        for repo in &ws.repos {
            println!("    - {} ({})", repo.name, repo.worktree_path);
        }
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

fn detect_tool(cmd: &str) -> Result<String> {
    crate::agents::resolve_tool_name(cmd)
        .map(|name| name.to_string())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown tool in command: {}\n\
                 Supported tools: {}\n\
                 Tip: Command must contain one of the supported tool names",
                cmd,
                crate::agents::agent_names().join(", ")
            )
        })
}
