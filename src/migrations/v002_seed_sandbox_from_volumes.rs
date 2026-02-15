//! Migration v002: Seed shared sandbox directories from existing named Docker volumes.
//!
//! Previously, agent auth was stored in named Docker volumes (e.g. `aoe-claude-auth`).
//! Now sandbox dirs are the only mechanism. This migration copies data from any
//! existing named volumes into the corresponding sandbox directories so users don't
//! lose their auth state.
//!
//! Old volumes are intentionally preserved after migration. Users can remove them
//! manually with `docker volume rm aoe-claude-auth aoe-opencode-auth ...`.

use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;
use tracing::info;

/// Mapping from legacy named volume to the host-relative sandbox directory.
struct VolumeMigration {
    volume_name: &'static str,
    /// Path relative to home where the sandbox dir lives (e.g. ".claude/sandbox").
    sandbox_rel: &'static str,
}

const VOLUME_MIGRATIONS: &[VolumeMigration] = &[
    VolumeMigration {
        volume_name: "aoe-claude-auth",
        sandbox_rel: ".claude/sandbox",
    },
    VolumeMigration {
        volume_name: "aoe-opencode-auth",
        sandbox_rel: ".local/share/opencode/sandbox",
    },
    VolumeMigration {
        volume_name: "aoe-codex-auth",
        sandbox_rel: ".codex/sandbox",
    },
    VolumeMigration {
        volume_name: "aoe-gemini-auth",
        sandbox_rel: ".gemini/sandbox",
    },
    VolumeMigration {
        volume_name: "aoe-vibe-auth",
        sandbox_rel: ".vibe/sandbox",
    },
];

/// Check whether Docker is available and the daemon is running.
fn docker_available() -> bool {
    Command::new("docker")
        .arg("info")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check whether a named Docker volume exists.
fn volume_exists(name: &str) -> bool {
    Command::new("docker")
        .args(["volume", "inspect", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check whether a directory is empty or does not exist.
fn dir_empty_or_missing(path: &PathBuf) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => true,
    }
}

/// Extract contents of a named volume into a host directory using a temporary container.
fn extract_volume(volume_name: &str, dest: &PathBuf) -> Result<()> {
    std::fs::create_dir_all(dest)?;

    let dest_str = dest.to_string_lossy();
    let output = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-v",
            &format!("{}:/vol", volume_name),
            "-v",
            &format!("{}:/host", dest_str),
            "alpine",
            "sh",
            "-c",
            "cp -a /vol/. /host/",
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "Failed to extract volume {}: {}",
            volume_name,
            stderr.trim()
        );
    }

    Ok(())
}

pub fn run() -> Result<()> {
    if !docker_available() {
        info!("Docker not available, skipping volume migration");
        return Ok(());
    }

    let Some(home) = dirs::home_dir() else {
        info!("Cannot determine home directory, skipping volume migration");
        return Ok(());
    };

    for migration in VOLUME_MIGRATIONS {
        if !volume_exists(migration.volume_name) {
            continue;
        }

        let sandbox_dir = home.join(migration.sandbox_rel);

        if !dir_empty_or_missing(&sandbox_dir) {
            info!(
                "Sandbox dir {} already has content, skipping volume {}",
                sandbox_dir.display(),
                migration.volume_name
            );
            continue;
        }

        info!(
            "Seeding {} from named volume {}",
            sandbox_dir.display(),
            migration.volume_name
        );

        if let Err(e) = extract_volume(migration.volume_name, &sandbox_dir) {
            tracing::warn!(
                "Failed to seed sandbox dir from volume {}: {}",
                migration.volume_name,
                e
            );
            // Continue with other volumes rather than failing the whole migration.
            continue;
        }

        info!(
            "Successfully seeded {} from volume {}",
            sandbox_dir.display(),
            migration.volume_name
        );
    }

    Ok(())
}
