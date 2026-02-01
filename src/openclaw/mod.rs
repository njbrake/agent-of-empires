//! OpenClaw Gateway integration module
//!
//! This module provides integration with OpenClaw Gateway for:
//! - Configuration management
//! - Cron job monitoring
//! - Channel binding

pub mod config;
pub mod gateway;

pub use config::OpenClawConfig;
pub use gateway::GatewayClient;
