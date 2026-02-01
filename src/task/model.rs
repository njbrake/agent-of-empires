//! Task data model

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Task ID in format T001, T002, etc.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub String);

impl TaskId {
    /// Create a new task ID from number
    pub fn from_number(n: u32) -> Self {
        Self(format!("T{:03}", n))
    }

    /// Parse task ID from string
    pub fn parse(s: &str) -> Option<Self> {
        if s.starts_with('T') && s.len() == 4 && s[1..].chars().all(|c| c.is_ascii_digit()) {
            Some(Self(s.to_string()))
        } else {
            None
        }
    }

    /// Get the numeric part
    pub fn number(&self) -> Option<u32> {
        self.0[1..].parse().ok()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Not started
    Todo,
    /// In progress
    Active,
    /// Waiting for something
    Blocked,
    /// Completed
    Done,
    /// Archived
    Archived,
}

impl TaskStatus {
    /// Parse status from emoji or text
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "⬜" | "todo" | "⏳" => Some(Self::Todo),
            "🔵" | "active" | "🔄" => Some(Self::Active),
            "🟡" | "blocked" | "⚠️" => Some(Self::Blocked),
            "✅" | "done" => Some(Self::Done),
            "📦" | "archived" => Some(Self::Archived),
            _ => None,
        }
    }

    /// Get the emoji for this status
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Todo => "⬜",
            Self::Active => "🔵",
            Self::Blocked => "🟡",
            Self::Done => "✅",
            Self::Archived => "📦",
        }
    }

    /// Get the text label
    pub fn label(&self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Active => "active",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Archived => "archived",
        }
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.emoji(), self.label())
    }
}

/// Task priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub enum TaskPriority {
    Low,
    #[default]
    Medium,
    High,
    Urgent,
}

impl TaskPriority {
    /// Parse priority from text
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "low" | "p3" => Some(Self::Low),
            "medium" | "med" | "p2" => Some(Self::Medium),
            "high" | "p1" => Some(Self::High),
            "urgent" | "p0" | "critical" => Some(Self::Urgent),
            _ => None,
        }
    }

    /// Get the label
    pub fn label(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Urgent => "urgent",
        }
    }
}

/// A task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique task ID
    pub id: TaskId,

    /// Task title
    pub title: String,

    /// Current status
    pub status: TaskStatus,

    /// Priority level
    #[serde(default)]
    pub priority: TaskPriority,

    /// Due date (if any)
    #[serde(default)]
    pub due: Option<NaiveDate>,

    /// Associated project ID
    #[serde(default)]
    pub project: Option<String>,

    /// Additional notes
    #[serde(default)]
    pub notes: Vec<String>,

    /// When the task was created
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,

    /// When the task was last updated
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,

    /// When the task was completed
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
}

impl Task {
    /// Create a new task
    pub fn new(id: TaskId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            status: TaskStatus::Todo,
            priority: TaskPriority::default(),
            due: None,
            project: None,
            notes: Vec::new(),
            created_at: Some(Utc::now()),
            updated_at: None,
            completed_at: None,
        }
    }

    /// Check if the task is overdue
    pub fn is_overdue(&self) -> bool {
        if let Some(due) = &self.due {
            let today = Utc::now().date_naive();
            due < &today && self.status != TaskStatus::Done && self.status != TaskStatus::Archived
        } else {
            false
        }
    }

    /// Check if the task is due today
    pub fn is_due_today(&self) -> bool {
        if let Some(due) = &self.due {
            let today = Utc::now().date_naive();
            due == &today
        } else {
            false
        }
    }

    /// Mark task as done
    pub fn complete(&mut self) {
        self.status = TaskStatus::Done;
        self.completed_at = Some(Utc::now());
        self.updated_at = Some(Utc::now());
    }

    /// Update the task
    pub fn touch(&mut self) {
        self.updated_at = Some(Utc::now());
    }

    /// Format as TASKS.md line
    pub fn to_markdown_line(&self) -> String {
        let mut line = format!("- {} **{}**: {}", self.status.emoji(), self.id, self.title);

        if let Some(project) = &self.project {
            line.push_str(&format!(" `{}`", project));
        }

        if let Some(due) = &self.due {
            line.push_str(&format!(" (due: {})", due.format("%Y-%m-%d")));
        }

        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_id() {
        let id = TaskId::from_number(42);
        assert_eq!(id.to_string(), "T042");
        assert_eq!(id.number(), Some(42));

        let parsed = TaskId::parse("T001");
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap().number(), Some(1));

        assert!(TaskId::parse("X001").is_none());
        assert!(TaskId::parse("T1").is_none());
    }

    #[test]
    fn test_task_status() {
        assert_eq!(TaskStatus::parse("✅"), Some(TaskStatus::Done));
        assert_eq!(TaskStatus::parse("todo"), Some(TaskStatus::Todo));
        assert_eq!(TaskStatus::Done.emoji(), "✅");
    }

    #[test]
    fn test_task_overdue() {
        let mut task = Task::new(TaskId::from_number(1), "Test");
        task.due = Some(NaiveDate::from_ymd_opt(2020, 1, 1).unwrap());
        assert!(task.is_overdue());

        task.complete();
        assert!(!task.is_overdue());
    }

    #[test]
    fn test_task_markdown() {
        let mut task = Task::new(TaskId::from_number(1), "Test task");
        task.project = Some("test-proj".to_string());
        task.due = Some(NaiveDate::from_ymd_opt(2026, 2, 15).unwrap());

        let line = task.to_markdown_line();
        assert!(line.contains("T001"));
        assert!(line.contains("Test task"));
        assert!(line.contains("`test-proj`"));
        assert!(line.contains("2026-02-15"));
    }
}
