//! Git endpoints: repository cloning and branch listing.

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use super::AppState;

// --- Clone repository ---

#[derive(Deserialize)]
pub struct CloneRepoBody {
    pub url: String,
    pub destination: Option<String>,
    #[serde(default)]
    pub shallow: bool,
}

/// Extract a repository name from a git URL.
///
/// Handles HTTPS, SSH, and scp-style URLs:
///   https://github.com/user/repo.git       -> repo
///   git@github.com:user/repo.git           -> repo
///   ssh://git@host/user/repo               -> repo
///   ssh://git@host:2222/user/repo.git      -> repo
fn repo_name_from_url(url: &str) -> Option<String> {
    // For scheme-based URLs (https://, ssh://, git://), take the last path segment.
    // For scp-style (git@host:path), split on ':' and take the last segment of the path.
    let last_segment = if url.contains("://") {
        url.rsplit_once('/')?.1
    } else if let Some((_host, path)) = url.split_once(':') {
        // scp-style: git@github.com:user/repo.git
        path.rsplit('/').next().unwrap_or(path)
    } else {
        url.rsplit_once('/')?.1
    };
    let name = last_segment.trim_end_matches(".git");
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

pub async fn clone_repo(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CloneRepoBody>,
) -> impl IntoResponse {
    if state.read_only {
        return (
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({"error": "read_only", "message": "Server is in read-only mode"}),
            ),
        )
            .into_response();
    }

    let url = body.url.trim().to_string();
    if url.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({"error": "validation_failed", "message": "URL cannot be empty"}),
            ),
        )
            .into_response();
    }

    // Basic URL scheme validation
    let looks_like_url = url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("git://")
        || url.starts_with("ssh://")
        || (url.contains('@') && url.contains(':') && !url.contains(' '));
    if !looks_like_url {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "validation_failed", "message": "URL does not look like a git repository URL"})),
        )
            .into_response();
    }

    // Resolve destination path
    let destination = if let Some(ref dest) = body.destination {
        let dest = dest.trim();
        if dest.is_empty() {
            None
        } else {
            Some(std::path::PathBuf::from(dest))
        }
    } else {
        None
    };

    let destination = match destination {
        Some(d) => d,
        None => {
            let repo_name = repo_name_from_url(&url).unwrap_or_else(|| "cloned-repo".to_string());
            let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
            home.join(&repo_name)
        }
    };

    // Security: destination must be within the home directory
    if let Some(home) = dirs::home_dir() {
        let canonical_home = home.canonicalize().unwrap_or(home);
        // For new paths that don't exist yet, check the parent
        let check_path = if destination.exists() {
            destination.canonicalize().unwrap_or(destination.clone())
        } else {
            destination
                .parent()
                .and_then(|p| p.canonicalize().ok())
                .map(|p| p.join(destination.file_name().unwrap_or_default()))
                .unwrap_or(destination.clone())
        };
        if !check_path.starts_with(&canonical_home) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "validation_failed", "message": "Destination must be within the home directory"})),
            )
                .into_response();
        }
    }

    let dest_display = destination.display().to_string();

    // Return an actionable error if the destination already exists, before
    // spawning the blocking task, so the user knows to pick a different name.
    if destination.exists() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "destination_exists",
                "message": format!(
                    "Directory '{}' already exists. Choose a different destination or delete it first.",
                    destination.file_name().unwrap_or_default().to_string_lossy()
                ),
                "path": dest_display,
            })),
        )
            .into_response();
    }

    let shallow = body.shallow;
    let result = tokio::task::spawn_blocking(move || {
        crate::git::clone_repo(&url, &destination, shallow)?;
        Ok::<String, crate::git::error::GitError>(destination.display().to_string())
    })
    .await;

    match result {
        Ok(Ok(path)) => {
            (StatusCode::CREATED, Json(serde_json::json!({"path": path}))).into_response()
        }
        Ok(Err(e)) => {
            let msg = e.to_string();
            tracing::warn!("Clone failed for {dest_display}: {msg}");
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "clone_failed", "message": msg})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Clone task panicked: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Internal server error"})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct BranchesQuery {
    pub path: String,
}

#[derive(Serialize)]
pub struct BranchInfo {
    pub name: String,
    pub is_current: bool,
}

pub async fn list_branches(
    axum::extract::Query(query): axum::extract::Query<BranchesQuery>,
) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        let path = std::path::Path::new(&query.path);
        if !crate::git::GitWorktree::is_git_repo(path) {
            return Err("Path is not a git repository".to_string());
        }

        let branches = crate::git::diff::list_branches(path).map_err(|e| e.to_string())?;

        let current = std::process::Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(path)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let mut result: Vec<BranchInfo> = branches
            .into_iter()
            .take(200)
            .map(|name| {
                let is_current = name == current;
                BranchInfo { name, is_current }
            })
            .collect();

        result.sort_by_key(|b| std::cmp::Reverse(b.is_current));

        Ok(result)
    })
    .await;

    match result {
        Ok(Ok(branches)) => (
            StatusCode::OK,
            Json(serde_json::to_value(branches).unwrap()),
        )
            .into_response(),
        Ok(Err(msg)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "not_a_repo", "message": msg})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal", "message": e.to_string()})),
        )
            .into_response(),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    // ── repo_name_from_url tests ─────────────────────────────────────────────

    #[test]
    fn repo_name_from_https_url() {
        assert_eq!(
            repo_name_from_url("https://github.com/user/my-repo.git"),
            Some("my-repo".to_string())
        );
    }

    #[test]
    fn repo_name_from_https_url_no_dotgit() {
        assert_eq!(
            repo_name_from_url("https://github.com/user/my-repo"),
            Some("my-repo".to_string())
        );
    }

    #[test]
    fn repo_name_from_scp_url() {
        assert_eq!(
            repo_name_from_url("git@github.com:user/my-repo.git"),
            Some("my-repo".to_string())
        );
    }

    #[test]
    fn repo_name_from_ssh_url() {
        assert_eq!(
            repo_name_from_url("ssh://git@github.com/user/my-repo.git"),
            Some("my-repo".to_string())
        );
    }

    #[test]
    fn repo_name_from_ssh_url_with_port() {
        assert_eq!(
            repo_name_from_url("ssh://git@host:2222/user/my-repo.git"),
            Some("my-repo".to_string())
        );
    }
}
