//! OpenClaw configuration types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Root OpenClaw configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawConfig {
    pub agents: AgentsConfig,
    #[serde(default)]
    pub channels: Option<ChannelsConfig>,
    #[serde(default)]
    pub tools: Option<ToolsConfig>,
}

/// Agents configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsConfig {
    pub workspace: PathBuf,
    #[serde(rename = "defaultModel")]
    pub default_model: String,
    #[serde(default)]
    pub list: HashMap<String, AgentConfig>,
}

/// Individual agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(rename = "systemPrompt")]
    pub system_prompt: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub tools: Option<AgentToolsConfig>,
}

/// Agent-specific tools configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolsConfig {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Channels configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelsConfig {
    #[serde(default)]
    pub slack: Option<SlackConfig>,
    #[serde(default)]
    pub telegram: Option<TelegramConfig>,
}

/// Slack channel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub enabled: bool,
    #[serde(default)]
    pub channels: Vec<SlackChannelConfig>,
}

/// Individual Slack channel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackChannelConfig {
    pub name: String,
    #[serde(rename = "agentId")]
    pub agent_id: String,
}

/// Telegram channel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub enabled: bool,
    #[serde(rename = "agentId")]
    pub agent_id: Option<String>,
}

/// Tools configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default)]
    pub exec: Option<ExecToolConfig>,
    #[serde(default)]
    pub sandbox: Option<SandboxToolConfig>,
}

/// Exec tool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecToolConfig {
    pub security: String,
}

/// Sandbox tool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxToolConfig {
    #[serde(default)]
    pub tools: SandboxToolsAllowDeny,
}

/// Sandbox tools allow/deny lists
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SandboxToolsAllowDeny {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Cron job configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: Option<String>,
    pub enabled: bool,
    pub schedule: CronSchedule,
    #[serde(rename = "sessionTarget")]
    pub session_target: String,
    pub payload: CronPayload,
    #[serde(default)]
    pub state: Option<CronJobState>,
}

/// Cron schedule configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronSchedule {
    pub kind: String,
    #[serde(default)]
    pub expr: Option<String>,
    #[serde(default)]
    pub tz: Option<String>,
}

/// Cron job payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronPayload {
    pub kind: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

/// Cron job execution state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobState {
    #[serde(rename = "nextRunAtMs")]
    pub next_run_at_ms: Option<i64>,
    #[serde(rename = "lastRunAtMs")]
    pub last_run_at_ms: Option<i64>,
    #[serde(rename = "lastStatus")]
    pub last_status: Option<String>,
    #[serde(rename = "lastError")]
    pub last_error: Option<String>,
    #[serde(rename = "lastDurationMs")]
    pub last_duration_ms: Option<i64>,
}

impl CronJob {
    /// Check if the job is healthy (last status was ok)
    pub fn is_healthy(&self) -> bool {
        self.state
            .as_ref()
            .and_then(|s| s.last_status.as_ref())
            .map(|s| s == "ok")
            .unwrap_or(false)
    }

    /// Get the job name for display
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}
