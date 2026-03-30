//! CLI commands for managing themes

use anyhow::{bail, Result};
use clap::Subcommand;

use crate::tui::styles::{
    available_themes, custom_themes_dir, export_theme_toml, load_theme, BUILTIN_THEMES,
};

#[derive(Subcommand)]
pub enum ThemeCommands {
    /// List all available themes (built-in and custom)
    #[command(alias = "ls")]
    List,

    /// Export a built-in theme as a TOML file for customization
    Export {
        /// Theme name to export
        name: String,

        /// Output file path (defaults to <name>.toml in the themes directory)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Show the custom themes directory path
    Dir,
}

pub fn run_list() {
    let themes = available_themes();
    let builtin_count = BUILTIN_THEMES.len();

    println!("Built-in themes:");
    for name in BUILTIN_THEMES {
        println!("  {}", name);
    }

    let custom: Vec<_> = themes.iter().skip(builtin_count).collect();
    if !custom.is_empty() {
        println!("\nCustom themes:");
        for name in &custom {
            println!("  {}", name);
        }
    }

    println!("\n{} built-in, {} custom", builtin_count, custom.len());
}

pub fn run_export(name: &str, output: Option<&str>) -> Result<()> {
    let theme = load_theme(name);

    // Verify the theme actually loaded (not a fallback due to unknown name)
    let all = available_themes();
    if !all.iter().any(|t| t == name) {
        bail!(
            "Unknown theme '{}'. Run `aoe theme list` to see available themes.",
            name
        );
    }

    let toml_str = export_theme_toml(&theme)?;

    match output {
        Some(path) => {
            std::fs::write(path, &toml_str)?;
            println!("Exported '{}' to {}", name, path);
        }
        None => {
            // Default: write to themes directory
            let dir = custom_themes_dir()
                .ok_or_else(|| anyhow::anyhow!("Cannot determine themes directory"))?;
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(format!("{}.toml", name));
            std::fs::write(&path, &toml_str)?;
            println!("Exported '{}' to {}", name, path.display());
        }
    }

    Ok(())
}

pub fn run_dir() {
    match custom_themes_dir() {
        Some(dir) => println!("{}", dir.display()),
        None => eprintln!("Cannot determine themes directory"),
    }
}
