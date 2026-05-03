//! `agent-of-empires template` subcommands implementation

use anyhow::{bail, Result};
use clap::Subcommand;

use crate::session::{load_templates, save_templates, find_template, upsert_template, remove_template, SessionTemplate, DEFAULT_TEMPLATES};

#[derive(Subcommand)]
pub enum TemplateCommands {
    /// List all templates
    #[command(alias = "ls")]
    List,

    /// Create a new template
    #[command(alias = "new")]
    Create {
        /// Template name
        name: String,
        /// Template description
        description: String,
        /// Command to run (optional)
        #[arg(long)]
        cmd: Option<String>,
        /// Environment variables (KEY=VALUE)
        #[arg(long)]
        env: Vec<String>,
    },

    /// Delete a template
    #[command(alias = "rm")]
    Delete {
        /// Template name
        name: String,
    },

    /// Show details of a template
    Show {
        /// Template name
        name: String,
    },
}

pub async fn run(command: Option<TemplateCommands>) -> Result<()> {
    match command {
        Some(TemplateCommands::List) | None => list_templates().await,
        Some(TemplateCommands::Create { name, description, cmd, env }) => create_template(name, description, cmd, env).await,
        Some(TemplateCommands::Delete { name }) => delete_template(&name).await,
        Some(TemplateCommands::Show { name }) => show_template(&name).await,
    }
}

async fn list_templates() -> Result<()> {
    let templates = load_templates()?;

    if templates.is_empty() {
        println!("No templates found.");
        println!("Default templates: debugging, refactoring, documentation");
        println!("Create with: aoe template create <name> --description <desc>");
        return Ok(());
    }

    println!("Templates:");
    for t in &templates {
        println!("  {}: {}", t.name, t.description);
        if let Some(cmd) = &t.cmd {
            println!("    Command: {}", cmd);
        }
        if let Some(env) = &t.env_vars {
            if !env.is_empty() {
                println!("    Environment: {} variables", env.len());
            }
        }
        println!();
    }
    println!("Total: {} templates", templates.len());

    Ok(())
}

async fn create_template(name: String, description: String, cmd: Option<String>, env: Vec<String>) -> Result<()> {
    if name.is_empty() {
        bail!("Template name cannot be empty");
    }

    let mut templates = load_templates()?;

    if find_template(&templates, &name).is_some() {
        bail!("Template '{}' already exists", name);
    }

    let env_vars = if env.is_empty() {
        None
    } else {
        let mut map = std::collections::HashMap::new();
        for e in env {
            if let Some((k, v)) = e.split_once('=') {
                map.insert(k.to_string(), v.to_string());
            } else {
                bail!("Invalid env format: {}", e);
            }
        }
        Some(map)
    };

    let template = SessionTemplate {
        name,
        description,
        cmd,
        env_vars,
    };

    templates = upsert_template(templates, template.clone());
    save_templates(&templates)?;

    println!("✓ Created template: {}", template.name);
    println!("  Description: {}", template.description);
    if let Some(cmd) = &template.cmd {
        println!("  Command: {}", cmd);
    }
    if let Some(env_vars) = &template.env_vars {
        println!("  Environment: {} variables", env_vars.len());
    }

    Ok(())
}

async fn delete_template(name: &str) -> Result<()> {
    let mut templates = load_templates()?;

    if find_template(&templates, name).is_none() {
        bail!("Template '{}' does not exist", name);
    }

    templates = remove_template(templates, name);
    save_templates(&templates)?;

    println!("✓ Deleted template: {}", name);

    Ok(())
}

async fn show_template(name: &str) -> Result<()> {
    let templates = load_templates()?;

    if let Some(template) = find_template(&templates, name) {
        println!("Name: {}", template.name);
        println!("Description: {}", template.description);
        if let Some(cmd) = &template.cmd {
            println!("Command: {}", cmd);
        } else {
            println!("Command: (none)");
        }
        if let Some(env_vars) = &template.env_vars {
            println!("Environment variables:");
            for (k, v) in env_vars {
                println!("  {}={}", k, v);
            }
        } else {
            println!("Environment variables: (none)");
        }
    } else {
        bail!("Template '{}' does not exist", name);
    }

    Ok(())
}