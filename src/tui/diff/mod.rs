//! Diff view - view changes against a base branch

mod input;
mod render;

use std::collections::HashMap;
use std::path::PathBuf;

use crate::git::diff::{
    check_merge_base_status, compute_changed_files, compute_file_diff, get_default_branch,
    list_branches, DiffFile, FileDiff,
};
use crate::session::config::{load_config, save_config};
use crate::session::Config;
use crate::tui::dialogs::InfoDialog;

pub use input::DiffAction;

/// State for branch selection dialog
#[derive(Debug, Clone, Default)]
pub struct BranchSelectState {
    pub branches: Vec<String>,
    pub selected: usize,
}

/// The diff view state
pub struct DiffView {
    /// Path to the repository root
    pub(crate) repo_path: PathBuf,

    /// Base branch to compare against
    pub(crate) base_branch: String,

    /// List of changed files
    pub(crate) files: Vec<DiffFile>,

    /// Currently selected file index
    pub(crate) selected_file: usize,

    /// Cached file diffs
    pub(crate) diff_cache: HashMap<PathBuf, FileDiff>,

    /// Scroll offset for the diff content
    pub(crate) scroll_offset: u16,

    /// Number of visible lines (set during render)
    pub(crate) visible_lines: u16,

    /// Total lines in current diff
    pub(crate) total_lines: u16,

    /// Branch selection dialog state
    pub(crate) branch_select: Option<BranchSelectState>,

    /// Error message to display
    pub(crate) error_message: Option<String>,

    /// Success message to display
    pub(crate) success_message: Option<String>,

    /// Context lines for diff
    pub(crate) context_lines: usize,

    /// Show help overlay
    pub(crate) show_help: bool,

    /// Width of the file list panel (resizable with h/l)
    pub(crate) file_list_width: u16,

    /// Warning dialog shown when merge-base can't be computed
    pub(crate) warning_dialog: Option<InfoDialog>,
}

impl DiffView {
    /// Create a new diff view for a repository
    pub fn new(repo_path: PathBuf) -> anyhow::Result<Self> {
        let config = Config::load().unwrap_or_default();

        // Determine base branch
        let base_branch = config
            .diff
            .default_branch
            .clone()
            .or_else(|| get_default_branch(&repo_path).ok())
            .unwrap_or_else(|| "main".to_string());

        let context_lines = config.diff.context_lines;

        let warning_dialog = check_merge_base_status(&repo_path, &base_branch)
            .map(|msg| InfoDialog::new("Warning", &msg));

        let mut view = Self {
            repo_path,
            base_branch,
            files: Vec::new(),
            selected_file: 0,
            diff_cache: HashMap::new(),
            scroll_offset: 0,
            visible_lines: 20,
            total_lines: 0,
            branch_select: None,
            error_message: None,
            success_message: None,
            context_lines,
            show_help: false,
            file_list_width: config.app_state.diff_file_list_width.unwrap_or(35),
            warning_dialog,
        };

        view.refresh_files()?;
        Ok(view)
    }

    /// Refresh the list of changed files
    pub fn refresh_files(&mut self) -> anyhow::Result<()> {
        self.files = compute_changed_files(&self.repo_path, &self.base_branch)?;
        self.diff_cache.clear();
        if self.selected_file >= self.files.len() {
            self.selected_file = self.files.len().saturating_sub(1);
        }
        self.scroll_offset = 0;
        Ok(())
    }

    /// Get the currently selected file
    pub fn selected_file(&self) -> Option<&DiffFile> {
        self.files.get(self.selected_file)
    }

    /// Get or compute the diff for the selected file
    pub fn get_current_diff(&mut self) -> Option<&FileDiff> {
        let file = self.files.get(self.selected_file)?;
        let path = file.path.clone();

        if !self.diff_cache.contains_key(&path) {
            match compute_file_diff(
                &self.repo_path,
                &path,
                &self.base_branch,
                self.context_lines,
            ) {
                Ok(diff) => {
                    self.diff_cache.insert(path.clone(), diff);
                }
                Err(e) => {
                    self.error_message = Some(format!("Failed to compute diff: {}", e));
                    return None;
                }
            }
        }

        self.diff_cache.get(&path)
    }

    /// Open the branch selection dialog
    pub fn open_branch_select(&mut self) {
        match list_branches(&self.repo_path) {
            Ok(branches) => {
                let selected = branches
                    .iter()
                    .position(|b| b == &self.base_branch)
                    .unwrap_or(0);
                self.branch_select = Some(BranchSelectState { branches, selected });
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to list branches: {}", e));
            }
        }
    }

    /// Select a branch and refresh
    pub fn select_branch(&mut self, branch: String) {
        self.base_branch = branch;
        self.branch_select = None;
        self.warning_dialog = check_merge_base_status(&self.repo_path, &self.base_branch)
            .map(|msg| InfoDialog::new("Warning", &msg));
        if let Err(e) = self.refresh_files() {
            self.error_message = Some(format!("Failed to refresh: {}", e));
        }
    }

    /// Navigate to next file
    pub fn next_file(&mut self) {
        if self.selected_file < self.files.len().saturating_sub(1) {
            self.selected_file += 1;
            self.scroll_offset = 0;
        }
    }

    /// Navigate to previous file
    pub fn prev_file(&mut self) {
        if self.selected_file > 0 {
            self.selected_file -= 1;
            self.scroll_offset = 0;
        }
    }

    /// Scroll diff content down
    pub fn scroll_down(&mut self, amount: u16) {
        let max_scroll = self.total_lines.saturating_sub(self.visible_lines);
        self.scroll_offset = (self.scroll_offset + amount).min(max_scroll);
    }

    /// Scroll diff content up
    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    /// Page down in diff content
    pub fn page_down(&mut self) {
        self.scroll_down(self.visible_lines.saturating_sub(2));
    }

    /// Page up in diff content
    pub fn page_up(&mut self) {
        self.scroll_up(self.visible_lines.saturating_sub(2));
    }

    /// Half-page down in diff content
    pub fn half_page_down(&mut self) {
        self.scroll_down(self.visible_lines / 2);
    }

    /// Half-page up in diff content
    pub fn half_page_up(&mut self) {
        self.scroll_up(self.visible_lines / 2);
    }

    /// Shrink the file list panel
    pub fn shrink_file_list(&mut self) {
        self.file_list_width = self.file_list_width.saturating_sub(5).max(5);
        self.save_file_list_width();
    }

    /// Grow the file list panel
    pub fn grow_file_list(&mut self) {
        self.file_list_width = (self.file_list_width + 5).min(80);
        self.save_file_list_width();
    }

    fn save_file_list_width(&self) {
        if let Ok(mut config) = load_config().map(|c| c.unwrap_or_default()) {
            config.app_state.diff_file_list_width = Some(self.file_list_width);
            let _ = save_config(&config);
        }
    }
}
