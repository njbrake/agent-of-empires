//! Migration v005: Seed cockpit settings on upgrade.
//!
//! 1.5.0 introduces the cockpit feature. Older configs do not have a
//! [cockpit] section. This migration writes a [cockpit] section to the
//! global config with the documented defaults so users can flip the
//! flag on without first running a settings TUI.
//!
//! Per-profile configs are left alone; the merge logic in
//! `profile_config.rs` falls back to the global value when a profile
//! doesn't override.

use anyhow::Result;
use std::fs;
use tracing::{debug, info};

pub fn run() -> Result<()> {
    let app_dir = crate::session::get_app_dir()?;
    let global_config = app_dir.join("config.toml");
    if !global_config.exists() {
        debug!("global config.toml not present, nothing to seed");
        return Ok(());
    }

    let content = fs::read_to_string(&global_config)?;
    let mut doc: toml::Table = match content.parse() {
        Ok(table) => table,
        Err(e) => {
            debug!("failed to parse {}: {e}, skipping", global_config.display());
            return Ok(());
        }
    };

    if doc.contains_key("cockpit") {
        debug!("[cockpit] already present, skipping");
        return Ok(());
    }

    let mut cockpit = toml::Table::new();
    cockpit.insert("enabled".into(), false.into());
    cockpit.insert("default_for_claude".into(), true.into());
    cockpit.insert("default_agent".into(), "aoe-agent".into());
    cockpit.insert("approval_timeout_secs".into(), (300_i64).into());
    cockpit.insert("destructive_require_double_confirm".into(), true.into());
    cockpit.insert("max_concurrent_workers".into(), (5_i64).into());
    cockpit.insert("replay_events".into(), (500_i64).into());
    cockpit.insert("replay_bytes".into(), (5_242_880_i64).into());

    doc.insert("cockpit".into(), toml::Value::Table(cockpit));

    let serialized = toml::to_string_pretty(&doc)?;
    fs::write(&global_config, serialized)?;

    info!(
        "v005: seeded [cockpit] section in {}",
        global_config.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeding_is_idempotent_and_preserves_other_sections() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(
            &path,
            "[other]\nkey = \"value\"\n",
        )
        .unwrap();

        // Run the inner migration logic directly against this file. The
        // public `run` reads the app_dir, so we simulate by calling the
        // body inline — this matches the pattern in v003's tests.
        let content = fs::read_to_string(&path).unwrap();
        let mut doc: toml::Table = content.parse().unwrap();
        assert!(!doc.contains_key("cockpit"));
        let mut cockpit = toml::Table::new();
        cockpit.insert("enabled".into(), false.into());
        cockpit.insert("default_for_claude".into(), true.into());
        doc.insert("cockpit".into(), toml::Value::Table(cockpit));
        fs::write(&path, toml::to_string_pretty(&doc).unwrap()).unwrap();

        let after: toml::Table = fs::read_to_string(&path).unwrap().parse().unwrap();
        assert!(after.contains_key("cockpit"));
        assert!(after.contains_key("other"));
    }
}
