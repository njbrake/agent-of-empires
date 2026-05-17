//! AVK git akış endpoint — FUR-4162.
//!
//! `GET /api/avk/git-flow` ajanlarim repo'nun açık ve son birleştirilmiş
//! PR'larını döner. Backend `gh` CLI'yi exec ederek GitHub'a HTTP/GraphQL
//! çağrısı atmak yerine local auth state'i kullanır (~/.config/gh/hosts.yml).
//!
//! Repo seçimi `AVK_GH_REPO` env (default `furkangurr/ajanlarim`).
//!
//! ## Tasarım kararı
//!
//! gh CLI exec kullanılır — Mac dev ortamı zaten authenticated; PAT/Token
//! env yönetimi gereksiz. VPS deploy'da `gh auth login --with-token`
//! gerekir, yoksa endpoint 503. Bu kabul edilebilir trade-off: dashboard
//! Mac-first çalışıyor, VPS PAT kurulumu Furkan'a tek seferlik.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use serde_json::Value;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use super::AppState;

const DEFAULT_REPO: &str = "furkangurr/ajanlarim";
const GH_TIMEOUT: Duration = Duration::from_secs(8);
const OPEN_LIMIT: u32 = 15;
const MERGED_LIMIT: u32 = 5;

#[derive(Debug, Serialize)]
pub struct PrSummary {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub url: String,
    pub updated_at: String,
    pub merged_at: Option<String>,
    pub mergeable: Option<String>,
    pub labels: Vec<String>,
    pub author: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GitFlowResponse {
    pub repo: String,
    pub open: Vec<PrSummary>,
    pub recent_merged: Vec<PrSummary>,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
}

pub async fn get_avk_git_flow(State(_state): State<Arc<AppState>>) -> Response {
    let repo = std::env::var("AVK_GH_REPO").unwrap_or_else(|_| DEFAULT_REPO.to_string());

    let open_result = run_gh_pr_list(&repo, "open", OPEN_LIMIT).await;
    let merged_result = run_gh_pr_list(&repo, "merged", MERGED_LIMIT).await;

    match (open_result, merged_result) {
        (Ok(open), Ok(recent_merged)) => Json(GitFlowResponse {
            repo,
            open,
            recent_merged,
        })
        .into_response(),
        (Err(e), _) | (_, Err(e)) => error_response(
            StatusCode::BAD_GATEWAY,
            &format!("gh CLI failed: {e}"),
            Some("gh_unavailable"),
        ),
    }
}

async fn run_gh_pr_list(repo: &str, state: &str, limit: u32) -> Result<Vec<PrSummary>, String> {
    let limit_s = limit.to_string();
    let fields = "number,title,state,url,updatedAt,mergedAt,mergeable,labels,author";

    let args = vec![
        "pr", "list", "--repo", repo, "--state", state, "--limit", &limit_s, "--json", fields,
    ];

    let exec = tokio::task::spawn_blocking({
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        move || {
            Command::new("gh")
                .args(&args)
                .output()
                .map_err(|e| format!("gh spawn: {e}"))
        }
    });

    let output = tokio::time::timeout(GH_TIMEOUT, exec)
        .await
        .map_err(|_| format!("gh CLI timeout after {}s", GH_TIMEOUT.as_secs()))?
        .map_err(|e| format!("gh task join: {e}"))??;

    if !output.status.success() {
        return Err(format!(
            "gh exit {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).map_err(|e| format!("gh JSON: {e}"))?;
    let arr = parsed
        .as_array()
        .ok_or_else(|| "gh JSON not an array".to_string())?;

    Ok(arr.iter().filter_map(parse_pr).collect())
}

fn parse_pr(node: &Value) -> Option<PrSummary> {
    let number = node.get("number")?.as_u64()?;
    let title = node.get("title")?.as_str()?.to_string();
    let state = node.get("state")?.as_str()?.to_string();
    let url = node
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let updated_at = node
        .get("updatedAt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let merged_at = node
        .get("mergedAt")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let mergeable = node
        .get("mergeable")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let labels = node
        .get("labels")
        .and_then(|l| l.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|l| {
                    l.get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    let author = node
        .get("author")
        .and_then(|a| a.get("login"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());

    Some(PrSummary {
        number,
        title,
        state,
        url,
        updated_at,
        merged_at,
        mergeable,
        labels,
        author,
    })
}

fn error_response(status: StatusCode, msg: &str, kind: Option<&'static str>) -> Response {
    (
        status,
        Json(ErrorBody {
            error: msg.to_string(),
            kind,
        }),
    )
        .into_response()
}
