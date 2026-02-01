//! Task management module
//!
//! This module provides task tracking with TASKS.md synchronization:
//! - Parse and write TASKS.md format
//! - Task state machine (todo -> active -> done)
//! - Due date tracking and alerts

pub mod model;
pub mod parser;

pub use model::{Task, TaskId, TaskPriority, TaskStatus};
pub use parser::TasksFile;
