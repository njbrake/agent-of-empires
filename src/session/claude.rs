//! Claude Code integration - session detection and forking

use anyhow::Result;
use std::fs;

pub fn detect_session_id(project_path: &str) -> Result<Option<String>> {
    let claude_dir = get_claude_projects_dir()?;

    // Hash the project path to find Claude's session directory
    let project_hash = hash_project_path(project_path);
    let session_dir = claude_dir.join(&project_hash);

    if !session_dir.exists() {
        return Ok(None);
    }

    // Look for the most recent session
    let chats_dir = session_dir.join("chats");
    if !chats_dir.exists() {
        return Ok(None);
    }

    // Find the most recently modified session file
    let mut latest: Option<(String, std::time::SystemTime)> = None;

    if let Ok(entries) = fs::read_dir(&chats_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(metadata) = path.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        let session_id = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .map(|s| s.to_string());

                        if let Some(id) = session_id {
                            if latest.is_none() || modified > latest.as_ref().unwrap().1 {
                                latest = Some((id, modified));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(latest.map(|(id, _)| id))
}

fn get_claude_projects_dir() -> Result<std::path::PathBuf> {
    // Check custom config dir first
    if let Some(custom_dir) = super::get_claude_config_dir() {
        return Ok(custom_dir.join("projects"));
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;

    // Default Claude config location
    Ok(home.join(".claude/projects"))
}

fn hash_project_path(path: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_project_path() {
        let hash1 = hash_project_path("/home/user/project");
        let hash2 = hash_project_path("/home/user/project");
        let hash3 = hash_project_path("/home/user/other");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }
}
