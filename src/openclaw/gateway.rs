//! OpenClaw Gateway client for interacting with the gateway configuration and API

use anyhow::{Context, Result};
use serde_json::Value;
use std::path::PathBuf;

/// Client for interacting with OpenClaw Gateway
pub struct GatewayClient {
    config_path: PathBuf,
}

impl GatewayClient {
    /// Create a new GatewayClient with the specified config path
    pub fn new(config_path: PathBuf) -> Self {
        Self { config_path }
    }

    /// Create a GatewayClient with the default config path (~/.openclaw/openclaw.json)
    pub fn with_default_path() -> Result<Self> {
        let home = dirs::home_dir().context("Could not find home directory")?;
        let config_path = home.join(".openclaw").join("openclaw.json");
        Ok(Self::new(config_path))
    }

    /// Check if the config file exists
    pub fn config_exists(&self) -> bool {
        self.config_path.exists()
    }

    /// Read the raw configuration file
    pub fn read_config(&self) -> Result<Value> {
        let content = std::fs::read_to_string(&self.config_path)
            .with_context(|| format!("Failed to read config from {:?}", self.config_path))?;
        serde_json::from_str(&content).context("Failed to parse config as JSON")
    }

    /// Get configuration for a specific agent
    pub fn get_agent(&self, agent_id: &str) -> Result<Option<Value>> {
        let config = self.read_config()?;
        Ok(config
            .get("agents")
            .and_then(|a| a.get("list"))
            .and_then(|l| l.get(agent_id))
            .cloned())
    }

    /// List all agent IDs
    pub fn list_agents(&self) -> Result<Vec<String>> {
        let config = self.read_config()?;
        let agents = config
            .get("agents")
            .and_then(|a| a.get("list"))
            .and_then(|l| l.as_object())
            .map(|obj| obj.keys().cloned().collect())
            .unwrap_or_default();
        Ok(agents)
    }

    /// Get the workspace path from config
    pub fn get_workspace(&self) -> Result<Option<PathBuf>> {
        let config = self.read_config()?;
        Ok(config
            .get("agents")
            .and_then(|a| a.get("workspace"))
            .and_then(|w| w.as_str())
            .map(PathBuf::from))
    }

    /// Get the default model from config
    pub fn get_default_model(&self) -> Result<Option<String>> {
        let config = self.read_config()?;
        Ok(config
            .get("agents")
            .and_then(|a| a.get("defaultModel"))
            .and_then(|m| m.as_str())
            .map(String::from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_config() -> Result<NamedTempFile> {
        let mut file = NamedTempFile::new()?;
        let config = r#"{
            "agents": {
                "workspace": "/test/workspace",
                "defaultModel": "claude-3-opus",
                "list": {
                    "main": {
                        "systemPrompt": "You are a helpful assistant"
                    },
                    "test-project": {
                        "systemPrompt": "Test project agent"
                    }
                }
            }
        }"#;
        file.write_all(config.as_bytes())?;
        Ok(file)
    }

    #[test]
    fn test_read_config() -> Result<()> {
        let file = create_test_config()?;
        let client = GatewayClient::new(file.path().to_path_buf());
        let config = client.read_config()?;
        assert!(config.get("agents").is_some());
        Ok(())
    }

    #[test]
    fn test_list_agents() -> Result<()> {
        let file = create_test_config()?;
        let client = GatewayClient::new(file.path().to_path_buf());
        let agents = client.list_agents()?;
        assert_eq!(agents.len(), 2);
        assert!(agents.contains(&"main".to_string()));
        assert!(agents.contains(&"test-project".to_string()));
        Ok(())
    }

    #[test]
    fn test_get_agent() -> Result<()> {
        let file = create_test_config()?;
        let client = GatewayClient::new(file.path().to_path_buf());

        let main = client.get_agent("main")?;
        assert!(main.is_some());

        let nonexistent = client.get_agent("nonexistent")?;
        assert!(nonexistent.is_none());

        Ok(())
    }

    #[test]
    fn test_get_workspace() -> Result<()> {
        let file = create_test_config()?;
        let client = GatewayClient::new(file.path().to_path_buf());
        let workspace = client.get_workspace()?;
        assert_eq!(workspace, Some(PathBuf::from("/test/workspace")));
        Ok(())
    }
}
