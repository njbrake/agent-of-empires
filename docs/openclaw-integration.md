# OpenClaw Integration Guide

This document outlines the integration of OpenClaw features into agent-of-empires.

## Overview

We are integrating the following OpenClaw capabilities:

1. **Project Context Management** - Automatic profile/environment switching
2. **Task Management** - TASKS.md synchronization with SQLite
3. **Schedule Engine** - Cron job and Heartbeat integration
4. **Dead Man's Switch** - Reliability monitoring
5. **Channel Binding** - Slack/Telegram/Discord integration

## Architecture Changes

### New Modules

```
src/
├── openclaw/           # NEW: OpenClaw Gateway integration
│   ├── mod.rs
│   ├── gateway.rs      # Gateway API client
│   ├── config.rs       # Configuration management
│   └── channel.rs      # Channel binding
│
├── project/            # NEW: Project context management
│   ├── mod.rs
│   ├── context.rs      # Context switching
│   ├── profile.rs      # Profile management
│   └── memory.rs       # Project memory handling
│
├── task/               # NEW: Task management
│   ├── mod.rs
│   ├── manager.rs      # CRUD operations
│   ├── sync.rs         # TASKS.md ↔ DB sync
│   └── state.rs        # State machine
│
├── schedule/           # NEW: Scheduler integration
│   ├── mod.rs
│   ├── engine.rs       # Schedule engine
│   ├── cron.rs         # OpenClaw Cron API
│   └── heartbeat.rs    # Heartbeat batch
│
└── monitor/            # NEW: Monitoring
    ├── mod.rs
    ├── dms.rs          # Dead Man's Switch
    ├── alert.rs        # Alert routing
    └── metrics.rs      # Prometheus export
```

### Configuration

#### Global Config (~/.config/openclaw-studio/config.toml)

```toml
[openclaw]
gateway_config = "~/.openclaw/openclaw.json"
workspace = "~/clawd"

[project]
default_profile = "scibit"
auto_switch = true

[monitor]
dms_enabled = true
alert_channel = "telegram"

[task]
sync_enabled = true
tasks_file = "TASKS.md"
```

#### Project Config (.openclaw/project.yaml)

Existing format from ocpm, directly compatible.

## Implementation Plan

### Phase 2: OpenClaw Integration

#### 2.1 Gateway Client (src/openclaw/gateway.rs)

```rust
use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;

pub struct GatewayClient {
    config_path: PathBuf,
}

impl GatewayClient {
    pub fn new(config_path: PathBuf) -> Self {
        Self { config_path }
    }
    
    pub fn read_config(&self) -> Result<Value> {
        let content = std::fs::read_to_string(&self.config_path)?;
        Ok(serde_json::from_str(&content)?)
    }
    
    pub fn get_agent(&self, agent_id: &str) -> Result<Option<Value>> {
        let config = self.read_config()?;
        // Navigate to agents.list[agent_id]
        Ok(config.get("agents")
            .and_then(|a| a.get("list"))
            .and_then(|l| l.get(agent_id))
            .cloned())
    }
    
    pub fn list_cron_jobs(&self) -> Result<Vec<CronJob>> {
        // Call cron list API
        todo!()
    }
}
```

#### 2.2 Project Context (src/project/context.rs)

```rust
use crate::session::profile_config::ProfileConfig;
use anyhow::Result;
use std::path::PathBuf;

pub struct ProjectContext {
    pub id: String,
    pub name: String,
    pub profile: String,
    pub browser_profile: Option<String>,
    pub memory_path: PathBuf,
}

impl ProjectContext {
    pub fn from_yaml(path: &PathBuf) -> Result<Self> {
        // Parse .openclaw/project.yaml
        todo!()
    }
    
    pub fn switch(&self) -> Result<()> {
        // 1. Switch profile (gcloud, env vars)
        // 2. Update .current-project
        // 3. Set environment variables
        todo!()
    }
}
```

### Phase 3: Orchestration

#### 3.1 Task Manager (src/task/manager.rs)

```rust
use rusqlite::Connection;
use anyhow::Result;

pub struct TaskManager {
    db: Connection,
    tasks_md: PathBuf,
}

impl TaskManager {
    pub fn list(&self, filter: TaskFilter) -> Result<Vec<Task>>;
    pub fn add(&self, task: NewTask) -> Result<TaskId>;
    pub fn update(&self, id: TaskId, patch: TaskPatch) -> Result<()>;
    pub fn done(&self, id: TaskId) -> Result<()>;
    pub fn sync(&self) -> Result<SyncResult>;
}
```

#### 3.2 Dead Man's Switch (src/monitor/dms.rs)

```rust
pub struct DeadManSwitch {
    jobs: HashMap<String, JobHealth>,
    grace_times: HashMap<String, Duration>,
}

impl DeadManSwitch {
    pub fn evaluate(&mut self, jobs: Vec<CronJob>) -> Vec<Alert> {
        jobs.iter().filter_map(|job| {
            let health = self.check_job(job);
            if health.state == JobState::Down {
                Some(Alert::new(job, health))
            } else {
                None
            }
        }).collect()
    }
}
```

## TUI Integration

### New Views

1. **Project Switcher** (`P` key) - Quick project context switch
2. **Task List** (`T` key) - View and manage tasks
3. **Monitor Dashboard** (`M` key) - Cron health status

### Status Bar

Add project context to status bar:
```
[expo-sns] 🟢 3 agents | T001 active | Cron ✅
```

## Migration Path

1. Install openclaw-studio alongside existing tools
2. Import existing ocpm projects
3. Migrate configurations
4. Deprecate ocpm CLI (optional)

## Testing

- Unit tests for each new module
- Integration tests for OpenClaw Gateway communication
- E2E tests for project context switching
- TUI tests for new views

---

*Last Updated: 2026-02-01*
