//! Project configuration types (.openclaw/project.yaml)

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Root project configuration from .openclaw/project.yaml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Unique project identifier (e.g., "expo-sns", "wmg2027")
    pub id: String,

    /// Human-readable project name
    pub name: String,

    /// Project status (active, on-hold, completed, archived)
    #[serde(default = "default_status")]
    pub status: String,

    /// Authentication profile to use (e.g., "scibit", "synthetiq")
    #[serde(default)]
    pub profile: Option<String>,

    /// Browser profile for web automation
    #[serde(default)]
    pub browser: Option<BrowserConfig>,

    /// Memory configuration
    #[serde(default)]
    pub memory: Option<MemoryConfig>,

    /// Customer information
    #[serde(default)]
    pub customer: Option<CustomerConfig>,

    /// Permissions and restrictions
    #[serde(default)]
    pub permissions: Option<PermissionsConfig>,

    /// External integrations
    #[serde(default)]
    pub integrations: Option<IntegrationsConfig>,
}

fn default_status() -> String {
    "active".to_string()
}

/// Browser automation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserConfig {
    /// Browser profile name (e.g., "clawd", "clawd-synthetiq")
    pub profile: String,
}

/// Memory isolation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Path to project-specific memory
    pub project: Option<PathBuf>,

    /// Path to shared/common memory
    pub common: Option<PathBuf>,

    /// Additional paths to index for memory search
    #[serde(default)]
    pub index_sources: Vec<PathBuf>,
}

/// Customer/client information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerConfig {
    /// Company name
    pub company: String,

    /// Key contacts
    #[serde(default)]
    pub contacts: Vec<ContactInfo>,

    /// Communication channels
    #[serde(default)]
    pub channels: HashMap<String, String>,
}

/// Contact information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactInfo {
    /// Contact name
    pub name: String,

    /// Contact role/title
    #[serde(default)]
    pub role: Option<String>,

    /// Email address
    #[serde(default)]
    pub email: Option<String>,
}

/// Permissions and access control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionsConfig {
    /// Allowed file paths (glob patterns)
    #[serde(default)]
    pub paths: Vec<String>,

    /// Allowed tools
    #[serde(default)]
    pub tools: ToolPermissions,

    /// Exec restrictions
    #[serde(default)]
    pub exec: Option<ExecPermissions>,
}

/// Tool permission settings
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolPermissions {
    /// Allowed tools
    #[serde(default)]
    pub allow: Vec<String>,

    /// Denied tools
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Exec command restrictions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecPermissions {
    /// Security mode (full, allowlist, deny)
    #[serde(default)]
    pub security: Option<String>,

    /// Allowed commands
    #[serde(default)]
    pub allow: Vec<String>,
}

/// External service integrations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IntegrationsConfig {
    /// Slack configuration
    #[serde(default)]
    pub slack: Option<SlackIntegration>,

    /// Jira configuration
    #[serde(default)]
    pub jira: Option<JiraIntegration>,

    /// Notion configuration
    #[serde(default)]
    pub notion: Option<NotionIntegration>,

    /// GitHub configuration
    #[serde(default)]
    pub github: Option<GitHubIntegration>,
}

/// Slack integration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackIntegration {
    /// Channel ID for project updates
    pub channel: Option<String>,

    /// Thread ID for ongoing discussions
    pub thread: Option<String>,
}

/// Jira integration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraIntegration {
    /// Jira project key
    pub project: String,

    /// Base URL
    pub base_url: Option<String>,
}

/// Notion integration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotionIntegration {
    /// Notion page/database ID
    pub page_id: Option<String>,

    /// Authentication method (api-key, mcp-oauth)
    pub method: Option<String>,
}

/// GitHub integration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubIntegration {
    /// Repository (owner/repo format)
    pub repo: String,
}

impl ProjectConfig {
    /// Load project configuration from a YAML file
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read project config from {:?}", path))?;
        Self::from_yaml(&content)
    }

    /// Parse project configuration from YAML string
    pub fn from_yaml(content: &str) -> Result<Self> {
        serde_yaml::from_str(content).context("Failed to parse project YAML")
    }

    /// Find and load project config from a directory (looks for .openclaw/project.yaml)
    pub fn from_directory(dir: &Path) -> Result<Option<Self>> {
        let config_path = dir.join(".openclaw").join("project.yaml");
        if config_path.exists() {
            Ok(Some(Self::from_file(&config_path)?))
        } else {
            Ok(None)
        }
    }

    /// Get the display name for the project (id + customer if available)
    pub fn display_name(&self) -> String {
        if let Some(customer) = &self.customer {
            format!("[{}] {} — {}", self.id, customer.company, self.name)
        } else {
            format!("[{}] {}", self.id, self.name)
        }
    }

    /// Check if the project is active
    pub fn is_active(&self) -> bool {
        self.status == "active"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_config() -> Result<()> {
        let yaml = r#"
id: test-project
name: Test Project
"#;
        let config = ProjectConfig::from_yaml(yaml)?;
        assert_eq!(config.id, "test-project");
        assert_eq!(config.name, "Test Project");
        assert_eq!(config.status, "active");
        Ok(())
    }

    #[test]
    fn test_parse_full_config() -> Result<()> {
        let yaml = r#"
id: expo-sns
name: SNS Analysis
status: active
profile: scibit
browser:
  profile: clawd
customer:
  company: Kansai Wide Area Union
  contacts:
    - name: Tanaka
      role: PM
      email: tanaka@example.com
  channels:
    slack: proj-expo-sns
permissions:
  paths:
    - /projects/expo-sns
  tools:
    allow:
      - browser
      - exec
integrations:
  github:
    repo: scibit/expo-sns
"#;
        let config = ProjectConfig::from_yaml(yaml)?;
        assert_eq!(config.id, "expo-sns");
        assert_eq!(config.profile, Some("scibit".to_string()));
        assert!(config.customer.is_some());
        assert!(config.permissions.is_some());
        assert!(config.integrations.is_some());
        Ok(())
    }

    #[test]
    fn test_display_name() -> Result<()> {
        let yaml = r#"
id: wmg2027
name: AI Chatbot
customer:
  company: WMG
"#;
        let config = ProjectConfig::from_yaml(yaml)?;
        assert_eq!(config.display_name(), "[wmg2027] WMG — AI Chatbot");
        Ok(())
    }
}
