//! Project context switching and management

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::config::ProjectConfig;

/// Represents the current project context
#[derive(Debug, Clone)]
pub struct ProjectContext {
    /// Project configuration
    pub config: ProjectConfig,

    /// Root directory of the project
    pub root: PathBuf,

    /// Environment variables to set for this project
    pub env: HashMap<String, String>,
}

/// Path to the current project marker file
const CURRENT_PROJECT_FILE: &str = ".current-project";

impl ProjectContext {
    /// Load project context from a directory
    pub fn from_directory(dir: &Path) -> Result<Option<Self>> {
        let config = ProjectConfig::from_directory(dir)?;
        match config {
            Some(config) => {
                let mut env = HashMap::new();

                // Set project ID environment variable
                env.insert("OPENCLAW_PROJECT_ID".to_string(), config.id.clone());

                // Set profile if specified
                if let Some(profile) = &config.profile {
                    env.insert("OPENCLAW_PROFILE".to_string(), profile.clone());
                }

                Ok(Some(Self {
                    config,
                    root: dir.to_path_buf(),
                    env,
                }))
            }
            None => Ok(None),
        }
    }

    /// Get the memory directory for this project
    pub fn memory_dir(&self) -> PathBuf {
        self.root.join(".openclaw").join("memory")
    }

    /// Get the project MEMORY.md path
    pub fn memory_file(&self) -> PathBuf {
        self.memory_dir().join("MEMORY.md")
    }

    /// Check if project memory exists
    pub fn has_memory(&self) -> bool {
        self.memory_file().exists()
    }

    /// Get the display status bar string
    pub fn status_bar(&self) -> String {
        let customer = self
            .config
            .customer
            .as_ref()
            .map(|c| c.company.as_str())
            .unwrap_or("—");

        format!(
            "🎯 [{}] {} — {} ｜ {}",
            self.config.id, customer, self.config.name, self.config.status
        )
    }
}

/// Manager for tracking and switching between projects
pub struct ProjectManager {
    /// Workspace root directory
    workspace: PathBuf,

    /// Currently active project
    current: Option<ProjectContext>,

    /// Cached project contexts
    projects: HashMap<String, ProjectContext>,
}

impl ProjectManager {
    /// Create a new project manager
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            current: None,
            projects: HashMap::new(),
        }
    }

    /// Load the current project from marker file
    pub fn load_current(&mut self) -> Result<Option<&ProjectContext>> {
        let marker_path = self.workspace.join(CURRENT_PROJECT_FILE);
        if !marker_path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&marker_path)?;
        let project_id = content.trim();

        if project_id.is_empty() {
            return Ok(None);
        }

        // Try to find and load the project
        if let Some(context) = self.projects.get(project_id) {
            self.current = Some(context.clone());
            return Ok(self.current.as_ref());
        }

        Ok(None)
    }

    /// Set the current project
    pub fn set_current(&mut self, project_id: &str) -> Result<()> {
        let marker_path = self.workspace.join(CURRENT_PROJECT_FILE);
        std::fs::write(&marker_path, project_id)
            .with_context(|| "Failed to write current project marker")?;

        if let Some(context) = self.projects.get(project_id) {
            self.current = Some(context.clone());
        }

        Ok(())
    }

    /// Clear the current project
    pub fn clear_current(&mut self) -> Result<()> {
        let marker_path = self.workspace.join(CURRENT_PROJECT_FILE);
        if marker_path.exists() {
            std::fs::remove_file(&marker_path)?;
        }
        self.current = None;
        Ok(())
    }

    /// Get the current project context
    pub fn current(&self) -> Option<&ProjectContext> {
        self.current.as_ref()
    }

    /// Scan a projects directory and load all project configs
    pub fn scan_projects(&mut self, projects_dir: &Path) -> Result<usize> {
        let mut count = 0;

        if !projects_dir.exists() {
            return Ok(0);
        }

        for entry in std::fs::read_dir(projects_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if let Some(context) = ProjectContext::from_directory(&path)? {
                    self.projects.insert(context.config.id.clone(), context);
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// List all known projects
    pub fn list(&self) -> Vec<&ProjectContext> {
        self.projects.values().collect()
    }

    /// Get a project by ID
    pub fn get(&self, project_id: &str) -> Option<&ProjectContext> {
        self.projects.get(project_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_project(dir: &Path, id: &str, name: &str) -> Result<()> {
        let openclaw_dir = dir.join(".openclaw");
        fs::create_dir_all(&openclaw_dir)?;

        let config = format!(
            r#"id: {}
name: {}
status: active
"#,
            id, name
        );

        fs::write(openclaw_dir.join("project.yaml"), config)?;
        Ok(())
    }

    #[test]
    fn test_project_context_from_directory() -> Result<()> {
        let temp = TempDir::new()?;
        create_test_project(temp.path(), "test-proj", "Test Project")?;

        let context = ProjectContext::from_directory(temp.path())?;
        assert!(context.is_some());

        let ctx = context.unwrap();
        assert_eq!(ctx.config.id, "test-proj");
        assert_eq!(ctx.config.name, "Test Project");
        assert_eq!(
            ctx.env.get("OPENCLAW_PROJECT_ID"),
            Some(&"test-proj".to_string())
        );

        Ok(())
    }

    #[test]
    fn test_status_bar() -> Result<()> {
        let temp = TempDir::new()?;

        let config = r#"id: expo-sns
name: SNS Analysis
status: active
customer:
  company: Kansai Union
"#;

        let openclaw_dir = temp.path().join(".openclaw");
        fs::create_dir_all(&openclaw_dir)?;
        fs::write(openclaw_dir.join("project.yaml"), config)?;

        let context = ProjectContext::from_directory(temp.path())?.unwrap();
        let status = context.status_bar();

        assert!(status.contains("[expo-sns]"));
        assert!(status.contains("Kansai Union"));
        assert!(status.contains("SNS Analysis"));
        assert!(status.contains("active"));

        Ok(())
    }

    #[test]
    fn test_project_manager() -> Result<()> {
        let temp = TempDir::new()?;
        let projects_dir = temp.path().join("projects");
        fs::create_dir_all(&projects_dir)?;

        // Create two test projects
        let proj1 = projects_dir.join("proj1");
        let proj2 = projects_dir.join("proj2");
        fs::create_dir_all(&proj1)?;
        fs::create_dir_all(&proj2)?;
        create_test_project(&proj1, "proj1", "Project One")?;
        create_test_project(&proj2, "proj2", "Project Two")?;

        let mut manager = ProjectManager::new(temp.path().to_path_buf());
        let count = manager.scan_projects(&projects_dir)?;

        assert_eq!(count, 2);
        assert_eq!(manager.list().len(), 2);
        assert!(manager.get("proj1").is_some());
        assert!(manager.get("proj2").is_some());

        Ok(())
    }
}
