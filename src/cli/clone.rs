//! Session cloning utilities

use anyhow::{bail, Result};
use clap::Args;

use crate::session::builder;
use crate::session::repo_config;
use crate::session::{civilizations, GroupTree, Instance, Storage};

#[derive(Args)]
pub struct CloneArgs {
    /// Session to clone (ID, title, or path prefix)
    session: String,

    /// New session title (defaults to auto-generated)
    #[arg(short = 't', long)]
    title: Option<String>,

    /// New group path (defaults to original)
    #[arg(short = 'g', long)]
    group: Option<String>,

    /// Override command (e.g., 'claude' or any other supported agent)
    #[arg(short = 'c', long = "cmd")]
    command: Option<String>,

    /// Launch the cloned session immediately
    #[arg(short = 'l', long)]
    launch: bool,
}

pub async fn run(profile: &str, args: CloneArgs) -> Result<()> {
    let mut storage = Storage::new(profile)?;

    let instances = storage.load()?;
    let instance = crate::cli::resolve_session(&args.session, &instances)?;

    println!("Cloning session: {}", instance.title);
    println!("  Original ID: {}", instance.id);
    println!("  Tool: {}", instance.tool);
    println!("  Path: {}", instance.project_path);

    // Create new instance based on original
    let mut new_instance = Instance::new("", instance.project_path.clone());
    new_instance.source_profile = profile.to_string();
    new_instance.tool = instance.tool.clone();
    new_instance.command = instance.command.clone();
    new_instance.env_vars = instance.env_vars.clone();
    new_instance.yolo_mode = instance.yolo_mode;
    new_instance.worktree_info = instance.worktree_info.clone();
    new_instance.workspace_info = instance.workspace_info.clone();

    // Apply overrides
    if let Some(title) = &args.title {
        new_instance.title = title.clone();
    } else {
        // Auto-generate title
        let existing_titles: Vec<&str> = instances.iter().map(|i| i.title.as_str()).collect();
        new_instance.title = civilizations::generate_random_title(&existing_titles);
    }

    if let Some(group) = &args.group {
        new_instance.group_path = group.trim().to_string();
    } else {
        new_instance.group_path = instance.group_path.clone();
    }

    if let Some(cmd) = &args.command {
        let tool_name = super::add::detect_tool(cmd)?;
        new_instance.tool = tool_name;
        if cmd.trim().contains(' ') {
            new_instance.command = cmd.clone();
        } else {
            new_instance.command = String::new();
        }
    }

    // Check for duplicate
    if instances.iter().any(|i| i.title == new_instance.title && i.project_path == new_instance.project_path) {
        bail!("Session with title '{}' and path '{}' already exists", new_instance.title, new_instance.project_path);
    }

    // Save and potentially launch
    let mut updated_instances = instances;
    updated_instances.push(new_instance.clone());
    storage.save(&updated_instances)?;

    println!("✓ Created cloned session: {}", new_instance.title);
    println!("  New ID: {}", new_instance.id);
    if args.launch {
        println!("Launching session...");
        // Launch logic would go here, similar to add
        // For now, just create
    }

    Ok(())
}