//! TASKS.md parser and writer

use anyhow::{Context, Result};
use regex::Regex;
use std::path::Path;

use super::model::{Task, TaskId, TaskPriority, TaskStatus};

/// Represents a parsed TASKS.md file
#[derive(Debug, Clone)]
pub struct TasksFile {
    /// All tasks organized by section
    pub sections: Vec<TaskSection>,
}

/// A section in TASKS.md (e.g., "Active", "Done")
#[derive(Debug, Clone)]
pub struct TaskSection {
    /// Section title (without ##)
    pub title: String,

    /// Tasks in this section
    pub tasks: Vec<Task>,
}

impl TasksFile {
    /// Parse TASKS.md from a file
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read TASKS.md from {:?}", path))?;
        Self::parse(&content)
    }

    /// Parse TASKS.md content
    pub fn parse(content: &str) -> Result<Self> {
        let mut sections = Vec::new();
        let mut current_section: Option<TaskSection> = None;

        // Regex for task line: - ✅ **T001**: Title `project` (due: 2026-02-15)
        let task_re = Regex::new(
            r"^-\s+(.)\s+\*\*([T]\d{3})\*\*:\s+(.+?)(?:\s+`([^`]+)`)?(?:\s+\(due:\s+(\d{4}-\d{2}-\d{2})\))?$"
        ).unwrap();

        for line in content.lines() {
            let line = line.trim();

            // Section header
            if line.starts_with("## ") {
                // Save previous section
                if let Some(section) = current_section.take() {
                    sections.push(section);
                }

                let title = line.trim_start_matches("## ").trim().to_string();
                current_section = Some(TaskSection {
                    title,
                    tasks: Vec::new(),
                });
                continue;
            }

            // Task line
            if let Some(caps) = task_re.captures(line) {
                let status_emoji = &caps[1];
                let id_str = &caps[2];
                let title = caps[3].trim();
                let project = caps.get(4).map(|m| m.as_str().to_string());
                let due_str = caps.get(5).map(|m| m.as_str());

                let status = TaskStatus::parse(status_emoji).unwrap_or(TaskStatus::Todo);
                let id = TaskId::parse(id_str).unwrap_or_else(|| TaskId(id_str.to_string()));

                let due =
                    due_str.and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

                let task = Task {
                    id,
                    title: title.to_string(),
                    status,
                    priority: TaskPriority::default(),
                    due,
                    project,
                    notes: Vec::new(),
                    created_at: None,
                    updated_at: None,
                    completed_at: None,
                };

                if let Some(section) = &mut current_section {
                    section.tasks.push(task);
                }
            }
        }

        // Save last section
        if let Some(section) = current_section {
            sections.push(section);
        }

        Ok(Self { sections })
    }

    /// Get all tasks
    pub fn all_tasks(&self) -> Vec<&Task> {
        self.sections.iter().flat_map(|s| &s.tasks).collect()
    }

    /// Get tasks by status
    pub fn by_status(&self, status: TaskStatus) -> Vec<&Task> {
        self.all_tasks()
            .into_iter()
            .filter(|t| t.status == status)
            .collect()
    }

    /// Get the next available task ID
    pub fn next_id(&self) -> TaskId {
        let max = self
            .all_tasks()
            .iter()
            .filter_map(|t| t.id.number())
            .max()
            .unwrap_or(0);
        TaskId::from_number(max + 1)
    }

    /// Get a task by ID
    pub fn get(&self, id: &TaskId) -> Option<&Task> {
        self.all_tasks().into_iter().find(|t| &t.id == id)
    }

    /// Find overdue tasks
    pub fn overdue(&self) -> Vec<&Task> {
        self.all_tasks()
            .into_iter()
            .filter(|t| t.is_overdue())
            .collect()
    }

    /// Find tasks due today
    pub fn due_today(&self) -> Vec<&Task> {
        self.all_tasks()
            .into_iter()
            .filter(|t| t.is_due_today())
            .collect()
    }

    /// Write to markdown format
    pub fn to_markdown(&self) -> String {
        let mut output = String::from("# TASKS.md\n\n");

        for section in &self.sections {
            output.push_str(&format!("## {}\n\n", section.title));

            for task in &section.tasks {
                output.push_str(&task.to_markdown_line());
                output.push('\n');
            }

            output.push('\n');
        }

        output
    }

    /// Write to file
    pub fn write_to_file(&self, path: &Path) -> Result<()> {
        let content = self.to_markdown();
        std::fs::write(path, content).with_context(|| format!("Failed to write to {:?}", path))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_TASKS: &str = r#"# TASKS.md

## Active

- 🔵 **T001**: Implement feature X `proj-a` (due: 2026-02-15)
- 🔵 **T002**: Review PR

## Todo

- ⬜ **T003**: Write documentation

## Done

- ✅ **T004**: Setup project
"#;

    #[test]
    fn test_parse_tasks() -> Result<()> {
        let file = TasksFile::parse(SAMPLE_TASKS)?;

        assert_eq!(file.sections.len(), 3);
        assert_eq!(file.sections[0].title, "Active");
        assert_eq!(file.sections[0].tasks.len(), 2);

        let task1 = &file.sections[0].tasks[0];
        assert_eq!(task1.id.to_string(), "T001");
        assert_eq!(task1.title, "Implement feature X");
        assert_eq!(task1.status, TaskStatus::Active);
        assert_eq!(task1.project, Some("proj-a".to_string()));
        assert!(task1.due.is_some());

        Ok(())
    }

    #[test]
    fn test_all_tasks() -> Result<()> {
        let file = TasksFile::parse(SAMPLE_TASKS)?;
        assert_eq!(file.all_tasks().len(), 4);
        Ok(())
    }

    #[test]
    fn test_by_status() -> Result<()> {
        let file = TasksFile::parse(SAMPLE_TASKS)?;
        assert_eq!(file.by_status(TaskStatus::Active).len(), 2);
        assert_eq!(file.by_status(TaskStatus::Todo).len(), 1);
        assert_eq!(file.by_status(TaskStatus::Done).len(), 1);
        Ok(())
    }

    #[test]
    fn test_next_id() -> Result<()> {
        let file = TasksFile::parse(SAMPLE_TASKS)?;
        assert_eq!(file.next_id().to_string(), "T005");
        Ok(())
    }

    #[test]
    fn test_roundtrip() -> Result<()> {
        let file = TasksFile::parse(SAMPLE_TASKS)?;
        let output = file.to_markdown();

        // Re-parse and verify
        let file2 = TasksFile::parse(&output)?;
        assert_eq!(file2.all_tasks().len(), file.all_tasks().len());

        Ok(())
    }
}
