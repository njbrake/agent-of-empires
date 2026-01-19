//! Data migrations for handling breaking changes across versions.
//!
//! Each migration is a one-time transformation that runs when upgrading from
//! an older version. Migrations are numbered sequentially and run in order.
//!
//! To add a new migration:
//! 1. Create a new module `vNNN_description.rs`
//! 2. Implement the migration function
//! 3. Add it to the `MIGRATIONS` array below

mod v001_xdg_linux;

use anyhow::Result;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info};

const CURRENT_VERSION: u32 = 1;
const VERSION_FILE: &str = ".schema_version";

struct Migration {
    version: u32,
    name: &'static str,
    run: fn() -> Result<()>,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "xdg_linux",
    run: v001_xdg_linux::run,
}];

/// Run all pending migrations. Call this early in app startup.
pub fn run_migrations() -> Result<()> {
    let current = get_current_version();
    debug!("Current schema version: {}", current);

    if current >= CURRENT_VERSION {
        return Ok(());
    }

    for migration in MIGRATIONS {
        if migration.version > current {
            info!(
                "Running migration v{:03}: {}",
                migration.version, migration.name
            );
            (migration.run)()?;
            set_version(migration.version)?;
        }
    }

    Ok(())
}

/// Get the current schema version by checking all possible locations.
fn get_current_version() -> u32 {
    for dir in get_all_possible_dirs() {
        let version_file = dir.join(VERSION_FILE);
        if let Ok(content) = fs::read_to_string(&version_file) {
            if let Ok(version) = content.trim().parse::<u32>() {
                return version;
            }
        }
    }
    0
}

/// Write the version to the current app directory.
fn set_version(version: u32) -> Result<()> {
    let dir = crate::session::get_app_dir()?;
    let version_file = dir.join(VERSION_FILE);
    fs::write(&version_file, version.to_string())?;
    debug!("Updated schema version to {}", version);
    Ok(())
}

/// Returns all directories where app data might exist (for migration discovery).
fn get_all_possible_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".agent-of-empires"));
    }

    #[cfg(target_os = "linux")]
    if let Some(config_dir) = dirs::config_dir() {
        dirs.push(config_dir.join("agent-of-empires"));
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrations_are_sequential() {
        let mut prev = 0;
        for m in MIGRATIONS {
            assert!(
                m.version > prev,
                "Migration {} should be > {}",
                m.version,
                prev
            );
            prev = m.version;
        }
    }

    #[test]
    fn test_current_version_matches_last_migration() {
        if let Some(last) = MIGRATIONS.last() {
            assert_eq!(CURRENT_VERSION, last.version);
        }
    }
}
