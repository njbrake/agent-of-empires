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

/// Bundled sound files (embedded at compile time)
const BUNDLED_SOUNDS: &[(&str, &[u8])] = &[
    ("start.wav", include_bytes!("../bundled_sounds/start.wav")),
    (
        "running.wav",
        include_bytes!("../bundled_sounds/running.wav"),
    ),
    (
        "waiting.wav",
        include_bytes!("../bundled_sounds/waiting.wav"),
    ),
    ("idle.wav", include_bytes!("../bundled_sounds/idle.wav")),
    ("error.wav", include_bytes!("../bundled_sounds/error.wav")),
    ("spell.wav", include_bytes!("../bundled_sounds/spell.wav")),
    ("coins.wav", include_bytes!("../bundled_sounds/coins.wav")),
    ("metal.wav", include_bytes!("../bundled_sounds/metal.wav")),
    ("chain.wav", include_bytes!("../bundled_sounds/chain.wav")),
    ("gem.wav", include_bytes!("../bundled_sounds/gem.wav")),
];

/// Get the directory where sound files are stored
pub fn get_sounds_dir() -> Option<PathBuf> {
    get_app_dir().ok().map(|d| d.join("sounds"))
}

/// Install bundled sounds to the user's config directory if they don't exist
pub fn install_bundled_sounds() -> Result<(), std::io::Error> {
    let Some(sounds_dir) = get_sounds_dir() else {
        return Ok(());
    };

    if !sounds_dir.exists() {
        std::fs::create_dir_all(&sounds_dir)?;
    }

    for (name, data) in BUNDLED_SOUNDS {
        let path = sounds_dir.join(name);
        if !path.exists() {
            std::fs::write(path, data)?;
            tracing::info!("Installed bundled sound: {}", name);
        }
    }

    Ok(())
}

/// List available sound files (names without extensions)
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
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    sounds.push(stem.to_string());
                }
            }
        }
    }
    sounds.sort();
    sounds
}

/// Find the full path for a sound by name (checks .wav then .ogg)
fn find_sound_file(name: &str) -> Option<PathBuf> {
    let dir = get_sounds_dir()?;
    let wav = dir.join(format!("{name}.wav"));
    if wav.exists() {
        return Some(wav);
    }
    let ogg = dir.join(format!("{name}.ogg"));
    if ogg.exists() {
        return Some(ogg);
    }
    None
}

/// Play a sound file by name (fire-and-forget, non-blocking)
pub fn play_sound(name: &str) {
    let Some(path) = find_sound_file(name) else {
        eprintln!("❌ Sound file not found: {}", name);
        tracing::debug!("Sound file not found: {}", name);
        return;
    };

    let path_str = path.to_string_lossy().to_string();
    eprintln!("🔊 Playing sound: {} from {}", name, path_str);

    std::thread::spawn(move || {
        let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
            ("afplay", vec![&path_str])
        } else {
            // Try paplay first (PulseAudio), fall back to aplay (ALSA)
            let ext = std::path::Path::new(&path_str)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("wav");

            if ext.eq_ignore_ascii_case("ogg") {
                ("paplay", vec![&path_str])
            } else {
                ("aplay", vec![&path_str])
            }
        };

        eprintln!("🎵 Executing: {} {}", cmd, args.join(" "));

        let result = std::process::Command::new(cmd)
            .args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    eprintln!("✓ Sound played successfully");
                } else {
                    eprintln!("❌ Command failed with exit code: {:?}", output.status.code());
                    if !output.stderr.is_empty() {
                        eprintln!("   stderr: {}", String::from_utf8_lossy(&output.stderr));
                    }
                }
            }
            Err(e) => {
                eprintln!("❌ Failed to execute command '{}': {}", cmd, e);
                eprintln!("   Is '{}' installed on your system?", cmd);
                tracing::debug!("Failed to play sound: {}", e);
            }
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
}
