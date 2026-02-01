//! Project context management module
//!
//! This module provides project-specific context handling for:
//! - Profile switching (GCP, browser, environment)
//! - Project configuration (.openclaw/project.yaml)
//! - Project memory isolation

pub mod config;
pub mod context;

pub use config::ProjectConfig;
pub use context::{ProjectContext, ProjectManager};
