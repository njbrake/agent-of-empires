//! Sound effects for agent state transitions
//!
//! Plays AoE II-style sounds when agent sessions change state.
//! Users place .wav/.ogg files in the sounds directory:
//!   - Linux: ~/.config/agent-of-empires/sounds/
//!   - macOS: ~/.agent-of-empires/sounds/
//!
//! Expected filenames (any .wav/.ogg file works):
//!   wololo.wav, rogan.wav, allhail.wav, monk.wav,
//!   alarm.wav, start.wav

use std::path::PathBuf;

use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

use crate::session::{get_app_dir, Status};

const GITHUB_SOUNDS_BASE_URL: &str =
    "https://raw.githubusercontent.com/njbrake/agent-of-empires/main/bundled_sounds";

/// How to select which sound file to play
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SoundMode {
    /// Pick a random sound from available files
    #[default]
    Random,
    /// Always play a specific sound file (by name, without extension)
    Specific(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SoundConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub mode: SoundMode,

    /// Sound to play when a session starts (overrides mode)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_start: Option<String>,

    /// Sound to play when a session enters running state
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_running: Option<String>,

    /// Sound to play when a session enters waiting state
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_waiting: Option<String>,

    /// Sound to play when a session enters idle state
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_idle: Option<String>,

    /// Sound to play when a session enters error state
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_error: Option<String>,
}

/// Profile override for sound config (all fields optional, None = inherit)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SoundConfigOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<SoundMode>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_start: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_running: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_waiting: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_idle: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_error: Option<String>,
}

/// List of bundled sound files available for download
const BUNDLED_SOUND_FILES: &[&str] = &[
    "start.wav",
    "running.wav",
    "waiting.wav",
    "idle.wav",
    "error.wav",
    "spell.wav",
    "coins.wav",
    "metal.wav",
    "chain.wav",
    "gem.wav",
];

/// Get the directory where sound files are stored
pub fn get_sounds_dir() -> Option<PathBuf> {
    get_app_dir().ok().map(|d| d.join("sounds"))
}

/// Download and install bundled sounds from GitHub
pub async fn install_bundled_sounds() -> anyhow::Result<()> {
    let Some(sounds_dir) = get_sounds_dir() else {
        return Err(anyhow::anyhow!("Could not determine sounds directory"));
    };

    if !sounds_dir.exists() {
        std::fs::create_dir_all(&sounds_dir)?;
    }

    let client = reqwest::Client::builder()
        .user_agent("agent-of-empires")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let mut failed = Vec::new();

    for filename in BUNDLED_SOUND_FILES {
        let path = sounds_dir.join(filename);
        if path.exists() {
            tracing::debug!("Sound already exists, skipping: {}", filename);
            continue;
        }

        let url = format!("{}/{}", GITHUB_SOUNDS_BASE_URL, filename);
        tracing::info!("Downloading sound: {}", filename);

        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => match response.bytes().await {
                Ok(bytes) => {
                    if let Err(e) = std::fs::write(&path, &bytes) {
                        tracing::warn!("Failed to write sound file {}: {}", filename, e);
                        failed.push(filename.to_string());
                    } else {
                        tracing::info!("Installed sound: {}", filename);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to download sound {}: {}", filename, e);
                    failed.push(filename.to_string());
                }
            },
            Ok(response) => {
                tracing::warn!(
                    "Failed to download {} (HTTP {})",
                    filename,
                    response.status()
                );
                failed.push(filename.to_string());
            }
            Err(e) => {
                tracing::warn!("Failed to download sound {}: {}", filename, e);
                failed.push(filename.to_string());
            }
        }
    }

    if !failed.is_empty() {
        return Err(anyhow::anyhow!(
            "Failed to download {} sound(s): {}",
            failed.len(),
            failed.join(", ")
        ));
    }

    Ok(())
}

/// List available sound files (names with extensions)
pub fn list_available_sounds() -> Vec<String> {
    let Some(dir) = get_sounds_dir() else {
        return Vec::new();
    };
    if !dir.exists() {
        return Vec::new();
    }

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut sounds = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext.eq_ignore_ascii_case("wav") || ext.eq_ignore_ascii_case("ogg") {
                if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                    sounds.push(filename.to_string());
                }
            }
        }
    }
    sounds.sort();
    sounds
}

/// Find the full path for a sound by filename (expects full filename with extension)
fn find_sound_file(filename: &str) -> Option<PathBuf> {
    let dir = get_sounds_dir()?;
    let path = dir.join(filename);
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

/// Validate that a sound file exists (for settings validation)
pub fn validate_sound_exists(filename: &str) -> Result<(), String> {
    if filename.is_empty() {
        return Ok(());
    }

    let available = list_available_sounds();
    if available.is_empty() {
        return Err(
            "No sounds installed. Run 'aoe sounds install' or add your own .wav/.ogg files."
                .to_string(),
        );
    }

    if !available.contains(&filename.to_string()) {
        return Err(format!(
            "Sound '{}' not found. Available sounds: {}",
            filename,
            available.join(", ")
        ));
    }

    Ok(())
}

/// Get the platform-specific audio command for playing a sound file
fn get_audio_command(path: &str) -> Result<(&'static str, Vec<&str>), std::io::Error> {
    if cfg!(target_os = "macos") {
        Ok(("afplay", vec![path]))
    } else {
        // Linux
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("wav");

        if ext.eq_ignore_ascii_case("ogg") {
            // Check if paplay is available
            if which_command("paplay").is_ok() {
                Ok(("paplay", vec![path]))
            } else if which_command("aplay").is_ok() {
                tracing::warn!("paplay not found, using aplay (may not support .ogg files)");
                Ok(("aplay", vec![path]))
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "No audio player found. Install alsa-utils (aplay) or pulseaudio-utils (paplay)",
                ))
            }
        } else {
            // WAV files
            if which_command("aplay").is_ok() {
                Ok(("aplay", vec![path]))
            } else if which_command("paplay").is_ok() {
                Ok(("paplay", vec![path]))
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "No audio player found. Install alsa-utils (aplay) or pulseaudio-utils (paplay)",
                ))
            }
        }
    }
}

/// Check if a command exists in PATH
fn which_command(cmd: &str) -> Result<(), std::io::Error> {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("{} not found", cmd),
                ))
            }
        })
}

/// Play a sound file by name (blocking version for testing)
pub fn play_sound_blocking(name: &str) -> Result<(), std::io::Error> {
    let Some(path) = find_sound_file(name) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Sound file not found: {}", name),
        ));
    };

    let path_str = path.to_string_lossy().to_string();
    let (cmd, args) = get_audio_command(&path_str)?;

    let output = std::process::Command::new(cmd)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "Sound playback failed with exit code: {:?}",
            output.status.code()
        )))
    }
}

/// Play a sound file by name (fire-and-forget, non-blocking)
pub fn play_sound(name: &str) {
    let Some(path) = find_sound_file(name) else {
        tracing::debug!("Sound file not found: {}", name);
        return;
    };

    let path_str = path.to_string_lossy().to_string();

    std::thread::spawn(move || {
        let (cmd, args) = match get_audio_command(&path_str) {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!("Audio player not available: {}", e);
                return;
            }
        };

        let result = std::process::Command::new(cmd)
            .args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();

        if let Err(e) = result {
            tracing::debug!("Failed to play sound: {}", e);
        }
    });
}

/// Resolve which sound name to play for the given config
fn resolve_sound_name(override_name: Option<&str>, config: &SoundConfig) -> Option<String> {
    // Per-transition override takes priority
    if let Some(name) = override_name {
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }

    match &config.mode {
        SoundMode::Specific(name) => Some(name.clone()),
        SoundMode::Random => {
            let sounds = list_available_sounds();
            if sounds.is_empty() {
                return None;
            }
            let mut rng = rand::thread_rng();
            sounds.choose(&mut rng).cloned()
        }
    }
}

/// Play a sound for a state transition (if enabled and sounds are available)
pub fn play_for_transition(old: Status, new: Status, config: &SoundConfig) {
    if !config.enabled || old == new {
        return;
    }

    let override_name = match new {
        Status::Starting => config.on_start.as_deref(),
        Status::Running => config.on_running.as_deref(),
        Status::Waiting => config.on_waiting.as_deref(),
        Status::Idle => config.on_idle.as_deref(),
        Status::Error => config.on_error.as_deref(),
        Status::Deleting => return, // No sound for deletion
    };

    if let Some(name) = resolve_sound_name(override_name, config) {
        play_sound(&name);
    }
}

/// Apply sound config overrides from a profile
pub fn apply_sound_overrides(target: &mut SoundConfig, source: &SoundConfigOverride) {
    if let Some(enabled) = source.enabled {
        target.enabled = enabled;
    }
    if let Some(ref mode) = source.mode {
        target.mode = mode.clone();
    }
    if source.on_start.is_some() {
        target.on_start = source.on_start.clone();
    }
    if source.on_running.is_some() {
        target.on_running = source.on_running.clone();
    }
    if source.on_waiting.is_some() {
        target.on_waiting = source.on_waiting.clone();
    }
    if source.on_idle.is_some() {
        target.on_idle = source.on_idle.clone();
    }
    if source.on_error.is_some() {
        target.on_error = source.on_error.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sound_config_default() {
        let config = SoundConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.mode, SoundMode::Random);
        assert!(config.on_start.is_none());
        assert!(config.on_running.is_none());
        assert!(config.on_waiting.is_none());
        assert!(config.on_idle.is_none());
        assert!(config.on_error.is_none());
    }

    #[test]
    fn test_sound_config_deserialize_empty() {
        let config: SoundConfig = toml::from_str("").unwrap();
        assert!(!config.enabled);
    }

    #[test]
    fn test_sound_config_deserialize() {
        let toml = r#"
            enabled = true
            mode = "random"
            on_error = "alarm"
        "#;
        let config: SoundConfig = toml::from_str(toml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.mode, SoundMode::Random);
        assert_eq!(config.on_error, Some("alarm".to_string()));
    }

    #[test]
    fn test_sound_mode_specific_deserialize() {
        let toml = r#"
            enabled = true
            mode = { specific = "wololo" }
        "#;
        let config: SoundConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.mode, SoundMode::Specific("wololo".to_string()));
    }

    #[test]
    fn test_sound_config_override_default() {
        let ovr = SoundConfigOverride::default();
        assert!(ovr.enabled.is_none());
        assert!(ovr.mode.is_none());
    }

    #[test]
    fn test_apply_sound_overrides() {
        let mut config = SoundConfig::default();
        let ovr = SoundConfigOverride {
            enabled: Some(true),
            on_error: Some("alarm".to_string()),
            ..Default::default()
        };
        apply_sound_overrides(&mut config, &ovr);
        assert!(config.enabled);
        assert_eq!(config.on_error, Some("alarm".to_string()));
        // Non-overridden fields stay default
        assert_eq!(config.mode, SoundMode::Random);
    }

    #[test]
    fn test_resolve_sound_name_override() {
        let config = SoundConfig {
            mode: SoundMode::Specific("default_sound".to_string()),
            ..Default::default()
        };
        let result = resolve_sound_name(Some("alarm"), &config);
        assert_eq!(result, Some("alarm".to_string()));
    }

    #[test]
    fn test_resolve_sound_name_specific_mode() {
        let config = SoundConfig {
            mode: SoundMode::Specific("wololo".to_string()),
            ..Default::default()
        };
        let result = resolve_sound_name(None, &config);
        assert_eq!(result, Some("wololo".to_string()));
    }

    #[test]
    fn test_resolve_sound_name_empty_override_uses_mode() {
        let config = SoundConfig {
            mode: SoundMode::Specific("wololo".to_string()),
            ..Default::default()
        };
        let result = resolve_sound_name(Some(""), &config);
        assert_eq!(result, Some("wololo".to_string()));
    }

    #[test]
    fn test_play_for_transition_disabled() {
        let config = SoundConfig::default();
        // Should not panic even when disabled
        play_for_transition(Status::Idle, Status::Running, &config);
    }

    #[test]
    fn test_play_for_transition_same_status() {
        let config = SoundConfig {
            enabled: true,
            mode: SoundMode::Specific("wololo".to_string()),
            ..Default::default()
        };
        // Same status - should be a no-op
        play_for_transition(Status::Running, Status::Running, &config);
    }

    #[test]
    fn test_play_for_transition_deleting_skipped() {
        let config = SoundConfig {
            enabled: true,
            mode: SoundMode::Specific("wololo".to_string()),
            ..Default::default()
        };
        // Deleting transitions should be skipped
        play_for_transition(Status::Running, Status::Deleting, &config);
    }

    #[test]
    fn test_validate_sound_exists_empty() {
        // Empty name should be valid
        assert!(validate_sound_exists("").is_ok());
    }

    #[test]
    fn test_validate_sound_exists_nonexistent() {
        // Non-existent sound should return error
        let result = validate_sound_exists("nonexistent_sound_xyz");
        assert!(result.is_err());
        if let Err(msg) = result {
            // Error should mention either no sounds installed or sound not found
            assert!(
                msg.contains("not found") || msg.contains("No sounds installed"),
                "Error message: {}",
                msg
            );
        }
    }
}
