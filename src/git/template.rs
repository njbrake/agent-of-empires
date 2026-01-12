// Path template system for worktrees

use std::path::PathBuf;

use super::error::Result;

pub struct TemplateVars {
    pub repo_name: String,
    pub branch: String,
    pub session_id: String,
    pub base_path: PathBuf,
}

pub fn sanitize_branch_name(branch: &str) -> String {
    branch.replace(
        ['/', '@', '#', '\\', ':', '*', '?', '"', '<', '>', '|'],
        "-",
    )
}

pub fn resolve_template(template: &str, vars: &TemplateVars) -> Result<PathBuf> {
    let sanitized_branch = sanitize_branch_name(&vars.branch);

    let resolved = template
        .replace("{repo-name}", &vars.repo_name)
        .replace("{branch}", &sanitized_branch)
        .replace("{session-id}", &vars.session_id);

    let path = if resolved.starts_with('/') {
        PathBuf::from(resolved)
    } else {
        vars.base_path.join(&resolved)
    };

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_template_with_repo_name() {
        let vars = TemplateVars {
            repo_name: "my-repo".to_string(),
            branch: "feat/test".to_string(),
            session_id: "abc123".to_string(),
            base_path: PathBuf::from("/home/user/repos/my-repo"),
        };

        let result = resolve_template("../{repo-name}-wt/{branch}", &vars).unwrap();
        assert!(result.to_string_lossy().contains("my-repo-wt"));
        assert!(result.to_string_lossy().contains("feat-test"));
    }

    #[test]
    fn test_sanitize_branch_name_replaces_slashes() {
        let sanitized = sanitize_branch_name("feat/my-feature");
        assert_eq!(sanitized, "feat-my-feature");
    }

    #[test]
    fn test_sanitize_branch_name_handles_special_chars() {
        let sanitized = sanitize_branch_name("feat@bug#123");
        assert!(!sanitized.contains("@"));
        assert!(!sanitized.contains("#"));
    }

    #[test]
    fn test_resolve_template_with_all_variables() {
        let vars = TemplateVars {
            repo_name: "test".to_string(),
            branch: "main".to_string(),
            session_id: "xyz789".to_string(),
            base_path: PathBuf::from("/repos/test"),
        };

        let result = resolve_template("../wt/{repo-name}/{branch}/{session-id}", &vars).unwrap();

        assert!(result.to_string_lossy().contains("test"));
        assert!(result.to_string_lossy().contains("main"));
        assert!(result.to_string_lossy().contains("xyz789"));
    }
}
