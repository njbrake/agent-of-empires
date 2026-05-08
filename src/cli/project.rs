//! `aoe project` subcommands: manage the project registry used by the
//! multi-repo workspace pickers.

use anyhow::{bail, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use std::path::PathBuf;

use crate::git::GitWorktree;
use crate::session::projects;
use crate::session::{Project, ProjectScope};

#[derive(Subcommand)]
pub enum ProjectCommands {
    /// List registered projects
    #[command(alias = "ls")]
    List(ProjectListArgs),

    /// Add a project to the registry
    Add(ProjectAddArgs),

    /// Remove a project from the registry
    #[command(alias = "rm")]
    Remove(ProjectRemoveArgs),
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ScopeFilter {
    All,
    Global,
    Profile,
}

#[derive(Args)]
pub struct ProjectListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,

    /// Filter by scope (default: all)
    #[arg(long, value_enum, default_value_t = ScopeFilter::All)]
    scope: ScopeFilter,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ScopeArg {
    Global,
    Profile,
}

#[derive(Args)]
pub struct ProjectAddArgs {
    /// Path to the git repository
    path: PathBuf,

    /// Display name (defaults to the directory's basename)
    #[arg(long)]
    name: Option<String>,

    /// Registry scope. Default: GLOBAL (visible from every profile).
    /// Pass `--scope profile` to scope the entry to the current profile only.
    #[arg(long, value_enum, default_value_t = ScopeArg::Global)]
    scope: ScopeArg,
}

#[derive(Args)]
pub struct ProjectRemoveArgs {
    /// Project name or path to remove
    name_or_path: String,

    /// Registry scope to remove from. Default: GLOBAL.
    #[arg(long, value_enum, default_value_t = ScopeArg::Global)]
    scope: ScopeArg,
}

#[derive(Serialize)]
struct ProjectInfo {
    name: String,
    path: String,
    scope: String,
}

pub async fn run(profile: &str, command: ProjectCommands) -> Result<()> {
    match command {
        ProjectCommands::List(args) => list(profile, args).await,
        ProjectCommands::Add(args) => add(profile, args).await,
        ProjectCommands::Remove(args) => remove(profile, args).await,
    }
}

async fn list(profile: &str, args: ProjectListArgs) -> Result<()> {
    let entries: Vec<Project> = match args.scope {
        ScopeFilter::All => projects::load_merged(profile)?,
        ScopeFilter::Global => projects::load_global()?,
        ScopeFilter::Profile => projects::load_profile(profile)?,
    };

    if args.json {
        let info: Vec<ProjectInfo> = entries
            .iter()
            .map(|p| ProjectInfo {
                name: p.name.clone(),
                path: p.path.clone(),
                scope: p.scope.as_str().to_string(),
            })
            .collect();
        super::output::print_json(&info)?;
        return Ok(());
    }

    if entries.is_empty() {
        println!("No projects registered.");
        println!("Add one with: aoe project add <path>");
        return Ok(());
    }

    println!("Projects:\n");
    for p in &entries {
        println!("  • {} [{}]  {}", p.name, p.scope.as_str(), p.path);
    }
    println!("\nTotal: {} projects", entries.len());
    Ok(())
}

async fn add(profile: &str, args: ProjectAddArgs) -> Result<()> {
    let scope = match args.scope {
        ScopeArg::Global => ProjectScope::Global,
        ScopeArg::Profile => ProjectScope::Profile,
    };

    let canonical = args
        .path
        .canonicalize()
        .unwrap_or_else(|_| args.path.clone());
    if !GitWorktree::is_git_repo(&canonical) {
        bail!(
            "Path is not a git repository: {}\n\
             Tip: pass the path to a repo's working tree (or a linked worktree)",
            canonical.display()
        );
    }

    let name = args.name.unwrap_or_else(|| {
        canonical
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string())
    });

    let project = Project::new(name.clone(), canonical.to_string_lossy(), scope);
    let saved = projects::add(profile, scope, project)?;
    println!(
        "✓ Registered project '{}' [{}] at {}",
        saved.name,
        scope.as_str(),
        saved.path
    );
    Ok(())
}

async fn remove(profile: &str, args: ProjectRemoveArgs) -> Result<()> {
    let scope = match args.scope {
        ScopeArg::Global => ProjectScope::Global,
        ScopeArg::Profile => ProjectScope::Profile,
    };
    let removed = projects::remove(profile, scope, &args.name_or_path)?;
    println!(
        "✓ Removed project '{}' [{}] (was at {})",
        removed.name,
        scope.as_str(),
        removed.path
    );
    Ok(())
}
