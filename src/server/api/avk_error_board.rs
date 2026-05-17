//! AVK Hata Ajanı panosu endpoint — FUR-4169.
//!
//! `GET /api/avk/error-board` Linear GraphQL'dan `Hata` label'ı taşıyan
//! issue'ları çeker (REFORM-A11 sonra "Hata Ajanı" rol label'ından AYRI
//! "bug" etiketi). Aktif (started + unstarted) + son 5 tamamlanmış (completed)
//! iki bölüm halinde döner; dashboard widget'ı dev hata durumunu özetler.
//!
//! `LINEAR_API_KEY` env reuse, 5sn timeout.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

use super::AppState;

const LINEAR_GRAPHQL_URL: &str = "https://api.linear.app/graphql";
const LINEAR_TIMEOUT: Duration = Duration::from_secs(5);
const BUG_LABEL: &str = "Hata";
const ACTIVE_LIMIT: u32 = 25;
const COMPLETED_LIMIT: u32 = 5;

const ACTIVE_QUERY: &str = r#"
query AvkBugsActive($first: Int!, $label: String!) {
  issues(
    first: $first
    filter: {
      labels: { name: { eq: $label } }
      state: { type: { in: ["started", "unstarted", "triage"] } }
    }
    orderBy: updatedAt
  ) {
    nodes {
      id
      identifier
      title
      priority
      priorityLabel
      state { name type }
      assignee { name }
      team { key }
      url
      updatedAt
      createdAt
    }
  }
}
"#;

const COMPLETED_QUERY: &str = r#"
query AvkBugsCompleted($first: Int!, $label: String!) {
  issues(
    first: $first
    filter: {
      labels: { name: { eq: $label } }
      state: { type: { eq: "completed" } }
    }
    orderBy: updatedAt
  ) {
    nodes {
      id
      identifier
      title
      priority
      priorityLabel
      state { name type }
      assignee { name }
      team { key }
      url
      updatedAt
      completedAt
    }
  }
}
"#;

#[derive(Debug, Serialize, Clone)]
pub struct BugIssue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub priority: u8,
    pub priority_label: String,
    pub state_name: String,
    pub state_type: String,
    pub assignee: Option<String>,
    pub team_key: Option<String>,
    pub url: String,
    pub updated_at: String,
    pub created_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ErrorBoardResponse {
    pub label: &'static str,
    pub active: Vec<BugIssue>,
    pub recently_resolved: Vec<BugIssue>,
    pub active_count: usize,
    pub resolved_count: usize,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
}

pub async fn get_avk_error_board(State(_state): State<Arc<AppState>>) -> Response {
    let Some(api_key) = load_api_key() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "LINEAR_API_KEY env yapılandırılmamış",
            Some("not_configured"),
        );
    };

    let active = fetch_bugs(&api_key, ACTIVE_QUERY, ACTIVE_LIMIT).await;
    let completed = fetch_bugs(&api_key, COMPLETED_QUERY, COMPLETED_LIMIT).await;

    match (active, completed) {
        (Ok(mut active), Ok(completed)) => {
            // Priority sort (1=Urgent en üstte, 0=No priority en alta).
            let rank = |p: u8| if p == 0 { u8::MAX } else { p };
            active.sort_by_key(|i| rank(i.priority));
            Json(ErrorBoardResponse {
                label: BUG_LABEL,
                active_count: active.len(),
                resolved_count: completed.len(),
                active,
                recently_resolved: completed,
            })
            .into_response()
        }
        (Err(e), _) | (_, Err(e)) => error_response(
            StatusCode::BAD_GATEWAY,
            &format!("Linear fail: {e}"),
            Some("upstream_error"),
        ),
    }
}

fn load_api_key() -> Option<String> {
    std::env::var("LINEAR_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
}

async fn fetch_bugs(api_key: &str, query: &str, first: u32) -> Result<Vec<BugIssue>, String> {
    let body = serde_json::json!({
        "query": query,
        "variables": { "first": first, "label": BUG_LABEL },
    });

    let client = reqwest::Client::builder()
        .timeout(LINEAR_TIMEOUT)
        .build()
        .map_err(|e| format!("reqwest build: {e}"))?;
    let resp = client
        .post(LINEAR_GRAPHQL_URL)
        .header("Authorization", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Linear unreachable: {e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Linear body read: {e}"))?;
    if !status.is_success() {
        return Err(format!("Linear HTTP {status}: {}", truncate(&text, 200)));
    }

    let parsed: Value = serde_json::from_str(&text).map_err(|e| format!("Linear parse: {e}"))?;
    if let Some(errors) = parsed.get("errors") {
        return Err(format!(
            "Linear GraphQL errors: {}",
            truncate(&errors.to_string(), 200)
        ));
    }

    let nodes = parsed
        .get("data")
        .and_then(|d| d.get("issues"))
        .and_then(|i| i.get("nodes"))
        .and_then(|n| n.as_array())
        .ok_or_else(|| "Linear response: data.issues.nodes missing".to_string())?;

    Ok(nodes.iter().filter_map(parse_bug).collect())
}

fn parse_bug(node: &Value) -> Option<BugIssue> {
    let id = node.get("id")?.as_str()?.to_string();
    let identifier = node.get("identifier")?.as_str()?.to_string();
    let title = node.get("title")?.as_str()?.to_string();
    let priority = node.get("priority").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
    let priority_label = node
        .get("priorityLabel")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let state = node.get("state")?;
    let state_name = state.get("name")?.as_str()?.to_string();
    let state_type = state.get("type")?.as_str()?.to_string();
    let assignee = node
        .get("assignee")
        .and_then(|a| a.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());
    let team_key = node
        .get("team")
        .and_then(|t| t.get("key"))
        .and_then(|k| k.as_str())
        .map(|s| s.to_string());
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
    let created_at = node
        .get("createdAt")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let completed_at = node
        .get("completedAt")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    Some(BugIssue {
        id,
        identifier,
        title,
        priority,
        priority_label,
        state_name,
        state_type,
        assignee,
        team_key,
        url,
        updated_at,
        created_at,
        completed_at,
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

fn truncate(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in s.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}
