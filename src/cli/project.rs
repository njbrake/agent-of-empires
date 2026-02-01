//! Project management CLI commands

use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::project::{ProjectContext, ProjectManager};

#[derive(Subcommand)]
pub enum ProjectCommands {
    /// List all projects
    List(ProjectListArgs),

    /// Show project details
    Show(ProjectShowArgs),

    /// Switch to a project context
    Switch(ProjectSwitchArgs),

    /// Show current project
    Current,

    /// Clear current project context
    Clear,
}

#[derive(Args)]
pub struct ProjectListArgs {
    /// Show only active projects
    #[arg(short, long)]
    active: bool,

    /// Projects directory to scan
    #[arg(short, long)]
    dir: Option<PathBuf>,
}

#[derive(Args)]
pub struct ProjectShowArgs {
    /// Project ID or path
    project: String,
}

#[derive(Args)]
pub struct ProjectSwitchArgs {
    /// Project ID to switch to
    project: String,
}

pub async fn run(profile: &str, command: ProjectCommands) -> Result<()> {
    match command {
        ProjectCommands::List(args) => run_list(profile, args).await,
        ProjectCommands::Show(args) => run_show(args).await,
        ProjectCommands::Switch(args) => run_switch(profile, args).await,
        ProjectCommands::Current => run_current(profile).await,
        ProjectCommands::Clear => run_clear(profile).await,
    }
}

async fn run_list(_profile: &str, args: ProjectListArgs) -> Result<()> {
    let workspace = get_workspace()?;
    let projects_dir = args.dir.unwrap_or_else(|| {
        // Default to ~/scibit/projects or similar
        dirs::home_dir()
            .map(|h| h.join("scibit").join("projects"))
            .unwrap_or_else(|| PathBuf::from("."))
    });

    let mut manager = ProjectManager::new(workspace);
    let count = manager.scan_projects(&projects_dir)?;

    if count == 0 {
        println!("No projects found in {:?}", projects_dir);
        return Ok(());
    }

    println!("Projects ({}):\n", count);

    for project in manager.list() {
        let status_emoji = match project.config.status.as_str() {
            "active" => "🟢",
            "on-hold" => "🟡",
            "completed" => "✅",
            "archived" => "📦",
            _ => "⬜",
        };

        if args.active && project.config.status != "active" {
            continue;
        }

        let customer = project
            .config
            .customer
            .as_ref()
            .map(|c| c.company.as_str())
            .unwrap_or("—");

        println!(
            "  {} [{}] {} — {}",
            status_emoji, project.config.id, customer, project.config.name
        );

        if let Some(profile) = &project.config.profile {
            println!("      profile: {}", profile);
        }
    }

    Ok(())
}

async fn run_show(args: ProjectShowArgs) -> Result<()> {
    let path = PathBuf::from(&args.project);

    let context: Option<ProjectContext> = if path.exists() {
        ProjectContext::from_directory(&path)?
    } else {
        // Try to find by ID
        let workspace = get_workspace()?;
        let projects_dir = dirs::home_dir()
            .map(|h| h.join("scibit").join("projects"))
            .unwrap_or_else(|| PathBuf::from("."));

        let mut manager = ProjectManager::new(workspace);
        manager.scan_projects(&projects_dir)?;

        manager.get(&args.project).cloned()
    };

    match context {
        Some(ctx) => {
            println!("{}\n", ctx.status_bar());
            println!("Path: {:?}", ctx.root);
            println!("Status: {}", ctx.config.status);

            if let Some(profile) = &ctx.config.profile {
                println!("Profile: {}", profile);
            }

            if let Some(customer) = &ctx.config.customer {
                println!("\nCustomer: {}", customer.company);
                for contact in &customer.contacts {
                    println!(
                        "  - {} ({})",
                        contact.name,
                        contact.role.as_deref().unwrap_or("")
                    );
                }
            }

            if ctx.has_memory() {
                println!("\nMemory: {:?}", ctx.memory_file());
            }

            Ok(())
        }
        None => {
            anyhow::bail!("Project not found: {}", args.project);
        }
    }
}

async fn run_switch(_profile: &str, args: ProjectSwitchArgs) -> Result<()> {
    let workspace = get_workspace()?;
    let projects_dir = dirs::home_dir()
        .map(|h| h.join("scibit").join("projects"))
        .unwrap_or_else(|| PathBuf::from("."));

    let mut manager = ProjectManager::new(workspace);
    manager.scan_projects(&projects_dir)?;

    if manager.get(&args.project).is_none() {
        anyhow::bail!("Project not found: {}", args.project);
    }

    manager.set_current(&args.project)?;

    let ctx = manager.current().unwrap();
    println!("Switched to: {}", ctx.status_bar());

    // Print environment variables to set
    for (key, value) in &ctx.env {
        println!("export {}=\"{}\"", key, value);
    }

    Ok(())
}

async fn run_current(_profile: &str) -> Result<()> {
    let workspace = get_workspace()?;
    let projects_dir = dirs::home_dir()
        .map(|h| h.join("scibit").join("projects"))
        .unwrap_or_else(|| PathBuf::from("."));

    let mut manager = ProjectManager::new(workspace);
    manager.scan_projects(&projects_dir)?;

    match manager.load_current()? {
        Some(ctx) => {
            println!("{}", ctx.status_bar());
        }
        None => {
            println!("No project selected");
        }
    }

    Ok(())
}

async fn run_clear(_profile: &str) -> Result<()> {
    let workspace = get_workspace()?;
    let mut manager = ProjectManager::new(workspace);
    manager.clear_current()?;
    println!("Project context cleared");
    Ok(())
}

fn get_workspace() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join("clawd"))
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))
}
