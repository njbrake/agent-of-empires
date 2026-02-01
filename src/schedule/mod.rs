//! Schedule and monitoring module
//!
//! This module provides integration with OpenClaw's scheduling features:
//! - Cron job monitoring
//! - Dead Man's Switch (health checks)
//! - Alert routing

pub mod cron;
pub mod dms;

pub use cron::{CronJobHealth, CronMonitor};
pub use dms::{DeadManSwitch, JobState};
