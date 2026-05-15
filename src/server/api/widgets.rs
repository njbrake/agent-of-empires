//! Widget summary endpoints (avk-suite custom adapt).
//!
//! Five REST GET endpoints under `/api/widgets/`:
//!   - `/linear/summary`         — In Progress + Backlog + 7d Done counts + 5 recent issue
//!   - `/sentry/summary`         — Last 24h unresolved issue count + 5 recent
//!   - `/github-actions/summary` — Multi-repo workflow runs status tally + cross-repo recent 5
//!   - `/vercel/summary`         — Deployment state bucket counts + recent 5
//!   - `/netdata/summary`        — Reachability + version + canonical chart URLs (iframe ready)
//!
//! Each backed by 60-second in-process cache to absorb dashboard refresh bursts
//! without hitting upstream rate limits. Env-driven (no silent fallback — missing
//! required env returns 500 with a precise error).
//!
//! Response header `x-cache: HIT|MISS` on success per docs/aoe-transplant/02-widget-api-contract.md.
//!
//! FUR-3957 transplant — port of the avk Sub-C/Sub-B-5+6 Next.js routes
//! (avk PRs ajan-sistemi#521 + #525). Contract doc:
//! `docs/aoe-transplant/02-widget-api-contract.md`.

use std::env;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::AppState;

const CACHE_TTL: Duration = Duration::from_secs(60);
const LINEAR_TEAM_ID: &str = "5669ce66-e92d-4a8a-96c3-49be9a68204f";
const LINEAR_GRAPHQL: &str = "https://api.linear.app/graphql";

// ─── Common cache ──────────────────────────────────────────────────────────

struct Cached<T> {
    data: T,
    expires_at: Instant,
}

#[derive(Default)]
pub struct WidgetCache {
    linear: Mutex<Option<Cached<LinearSummary>>>,
    sentry: Mutex<Option<Cached<SentrySummary>>>,
    github_actions: Mutex<Option<Cached<GitHubActionsSummary>>>,
    vercel: Mutex<Option<Cached<VercelSummary>>>,
    netdata: Mutex<Option<Cached<NetdataSummary>>>,
}

impl WidgetCache {
    pub fn new() -> Self {
        Self::default()
    }
}

macro_rules! cache_accessors {
    ($name:ident, $get:ident, $set:ident, $ty:ty) => {
        impl WidgetCache {
            fn $get(&self) -> Option<$ty> {
                let guard = self.$name.lock().ok()?;
                let entry = guard.as_ref()?;
                if entry.expires_at > Instant::now() {
                    Some(entry.data.clone())
                } else {
                    None
                }
            }

            fn $set(&self, data: $ty) {
                if let Ok(mut guard) = self.$name.lock() {
                    *guard = Some(Cached {
                        data,
                        expires_at: Instant::now() + CACHE_TTL,
                    });
                }
            }
        }
    };
}

cache_accessors!(linear, get_linear, set_linear, LinearSummary);
cache_accessors!(sentry, get_sentry, set_sentry, SentrySummary);
cache_accessors!(github_actions, get_gh, set_gh, GitHubActionsSummary);
cache_accessors!(vercel, get_vercel, set_vercel, VercelSummary);
cache_accessors!(netdata, get_netdata, set_netdata, NetdataSummary);

/// Response helper — attach `x-cache` header on success.
fn with_cache_header<T: Serialize>(data: T, hit: bool) -> axum::response::Response {
    let header = if hit { "HIT" } else { "MISS" };
    ([("x-cache", header)], Json(data)).into_response()
}

// ─── Linear ────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct LinearCount {
    pub count: usize,
    pub has_more: bool,
}

#[derive(Clone, Serialize)]
pub struct LinearIssue {
    pub identifier: String,
    pub title: String,
    pub state: String,
    pub url: String,
    pub updated_at: String,
}

#[derive(Clone, Serialize)]
pub struct LinearSummary {
    pub in_progress: LinearCount,
    pub backlog: LinearCount,
    pub done7d: LinearCount,
    pub recent: Vec<LinearIssue>,
    pub fetched_at: String,
}

#[derive(Deserialize)]
struct LinearGraphResp {
    data: Option<LinearGraphData>,
    errors: Option<Vec<LinearGraphError>>,
}

#[derive(Deserialize)]
struct LinearGraphError {
    message: String,
}

#[derive(Deserialize)]
struct LinearGraphData {
    #[serde(rename = "inProgress")]
    in_progress: LinearConn,
    backlog: LinearConn,
    #[serde(rename = "done7d")]
    done7d: LinearConn,
    recent: LinearRecentConn,
}

#[derive(Deserialize)]
struct LinearConn {
    nodes: Vec<Value>,
    #[serde(rename = "pageInfo")]
    page_info: LinearPageInfo,
}

#[derive(Deserialize)]
struct LinearPageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
}

#[derive(Deserialize)]
struct LinearRecentConn {
    nodes: Vec<LinearRecentNode>,
}

#[derive(Deserialize)]
struct LinearRecentNode {
    identifier: String,
    title: String,
    url: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    state: LinearStateRef,
}

#[derive(Deserialize)]
struct LinearStateRef {
    name: String,
}

const LINEAR_QUERY: &str = r#"
query Summary($teamId: ID!, $since: DateTimeOrDuration!) {
  inProgress: issues(filter: {team: {id: {eq: $teamId}}, state: {type: {eq: "started"}}}, first: 250) {
    nodes { id }
    pageInfo { hasNextPage }
  }
  backlog: issues(filter: {team: {id: {eq: $teamId}}, state: {type: {in: ["backlog", "unstarted"]}}}, first: 250) {
    nodes { id }
    pageInfo { hasNextPage }
  }
  done7d: issues(filter: {team: {id: {eq: $teamId}}, state: {type: {eq: "completed"}}, completedAt: {gte: $since}}, first: 250) {
    nodes { id }
    pageInfo { hasNextPage }
  }
  recent: issues(filter: {team: {id: {eq: $teamId}}}, orderBy: updatedAt, first: 5) {
    nodes {
      identifier
      title
      url
      updatedAt
      state { name }
    }
  }
}
"#;

async fn fetch_linear(api_key: &str) -> Result<LinearSummary, String> {
    let since = chrono::Utc::now() - chrono::Duration::days(7);
    let since_iso = since.to_rfc3339();
    let body = serde_json::json!({
        "query": LINEAR_QUERY,
        "variables": { "teamId": LINEAR_TEAM_ID, "since": since_iso }
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(LINEAR_GRAPHQL)
        .header("Authorization", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Linear request error: {e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Linear body read error: {e}"))?;

    if !status.is_success() {
        return Err(format!("Linear API {status}: {text}"));
    }

    let parsed: LinearGraphResp = serde_json::from_str(&text)
        .map_err(|e| format!("Linear JSON parse error: {e} | body: {text}"))?;

    if let Some(errs) = parsed.errors {
        let msg = errs
            .into_iter()
            .map(|e| e.message)
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!("Linear GraphQL error: {msg}"));
    }

    let data = parsed.data.ok_or_else(|| "Linear data missing".to_string())?;

    Ok(LinearSummary {
        in_progress: LinearCount {
            count: data.in_progress.nodes.len(),
            has_more: data.in_progress.page_info.has_next_page,
        },
        backlog: LinearCount {
            count: data.backlog.nodes.len(),
            has_more: data.backlog.page_info.has_next_page,
        },
        done7d: LinearCount {
            count: data.done7d.nodes.len(),
            has_more: data.done7d.page_info.has_next_page,
        },
        recent: data
            .recent
            .nodes
            .into_iter()
            .map(|n| LinearIssue {
                identifier: n.identifier,
                title: n.title,
                state: n.state.name,
                url: n.url,
                updated_at: n.updated_at,
            })
            .collect(),
        fetched_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub async fn get_linear_summary(
    State(state): State<Arc<AppState>>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let api_key = env::var("LINEAR_API_KEY").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "LINEAR_API_KEY env eksik".to_string(),
        )
    })?;

    if let Some(cached) = state.widget_cache.get_linear() {
        return Ok(with_cache_header(cached, true));
    }

    match fetch_linear(&api_key).await {
        Ok(data) => {
            state.widget_cache.set_linear(data.clone());
            Ok(with_cache_header(data, false))
        }
        Err(msg) => Err((StatusCode::BAD_GATEWAY, msg)),
    }
}

// ─── Sentry ────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct SentryIssue {
    pub id: String,
    pub short_id: String,
    pub title: String,
    pub culprit: String,
    pub count: String,
    pub permalink: String,
    pub last_seen: String,
}

#[derive(Clone, Serialize)]
pub struct SentrySummary {
    pub total_issues: usize,
    pub unresolved: usize,
    pub recent: Vec<SentryIssue>,
    pub fetched_at: String,
}

#[derive(Deserialize)]
struct SentryRaw {
    id: String,
    #[serde(rename = "shortId")]
    short_id: String,
    title: String,
    culprit: String,
    count: String,
    permalink: String,
    #[serde(rename = "lastSeen")]
    last_seen: String,
    status: String,
}

/// Sentry slug/org validation: alphanumeric + dash/underscore/dot. Sentry
/// slugs by convention are lowercase kebab; we reject anything outside that
/// alphabet so the URL stays well-formed without a percent-encoder.
fn valid_sentry_slug(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

async fn fetch_sentry(
    auth_token: &str,
    org: &str,
    project_slug: &str,
) -> Result<SentrySummary, String> {
    if !valid_sentry_slug(org) || !valid_sentry_slug(project_slug) {
        return Err(
            "Sentry org/project_slug geçersiz karakter içeriyor (alphanumeric + -_. izinli)"
                .to_string(),
        );
    }

    let url = format!(
        "https://sentry.io/api/0/projects/{org}/{project_slug}/issues/?statsPeriod=24h&limit=25&sort=date&query=is:unresolved"
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {auth_token}"))
        .send()
        .await
        .map_err(|e| format!("Sentry request error: {e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Sentry body read error: {e}"))?;

    if !status.is_success() {
        return Err(format!("Sentry API {status}: {text}"));
    }

    let raw: Vec<SentryRaw> = serde_json::from_str(&text)
        .map_err(|e| format!("Sentry JSON parse error: {e}"))?;

    let unresolved: Vec<&SentryRaw> = raw.iter().filter(|r| r.status == "unresolved").collect();
    let total = raw.len();
    let unresolved_count = unresolved.len();

    Ok(SentrySummary {
        total_issues: total,
        unresolved: unresolved_count,
        recent: unresolved
            .into_iter()
            .take(5)
            .map(|r| SentryIssue {
                id: r.id.clone(),
                short_id: r.short_id.clone(),
                title: r.title.clone(),
                culprit: r.culprit.clone(),
                count: r.count.clone(),
                permalink: r.permalink.clone(),
                last_seen: r.last_seen.clone(),
            })
            .collect(),
        fetched_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub async fn get_sentry_summary(
    State(state): State<Arc<AppState>>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let auth = env::var("SENTRY_AUTH_TOKEN").ok();
    let org = env::var("SENTRY_ORG").ok();
    let project = env::var("SENTRY_PROJECT_SLUG").ok();

    let (auth, org, project) = match (auth, org, project) {
        (Some(a), Some(o), Some(p)) if !a.is_empty() && !o.is_empty() && !p.is_empty() => {
            (a, o, p)
        }
        _ => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Sentry env eksik (SENTRY_AUTH_TOKEN + SENTRY_ORG + SENTRY_PROJECT_SLUG gerekli)"
                    .to_string(),
            ));
        }
    };

    if let Some(cached) = state.widget_cache.get_sentry() {
        return Ok(with_cache_header(cached, true));
    }

    match fetch_sentry(&auth, &org, &project).await {
        Ok(data) => {
            state.widget_cache.set_sentry(data.clone());
            Ok(with_cache_header(data, false))
        }
        Err(msg) => Err((StatusCode::BAD_GATEWAY, msg)),
    }
}

// ─── GitHub Actions ────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct GitHubRun {
    pub id: u64,
    pub repo: String,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub html_url: String,
    pub head_branch: String,
    pub event: String,
    pub run_started_at: String,
    pub updated_at: String,
}

#[derive(Clone, Serialize)]
pub struct GitHubRepoStats {
    pub repo: String,
    pub success: usize,
    pub failure: usize,
    pub in_progress: usize,
    pub other: usize,
}

#[derive(Clone, Serialize)]
pub struct GitHubActionsSummary {
    pub repos: Vec<GitHubRepoStats>,
    pub recent: Vec<GitHubRun>,
    pub fetched_at: String,
}

#[derive(Deserialize)]
struct GitHubRawRun {
    id: u64,
    name: Option<String>,
    status: String,
    conclusion: Option<String>,
    html_url: String,
    head_branch: Option<String>,
    event: String,
    run_started_at: Option<String>,
    updated_at: String,
}

#[derive(Deserialize)]
struct GitHubWorkflowRunsResp {
    workflow_runs: Vec<GitHubRawRun>,
}

/// Parse comma-separated repos, trimmed + `owner/name` shape filter.
pub fn parse_repos(env: &str) -> Vec<String> {
    env.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.contains('/'))
        .collect()
}

/// Validate repo slug: only `owner/name` characters (alphanumeric + `_-./`).
fn valid_repo_slug(s: &str) -> bool {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() != 2 {
        return false;
    }
    parts.iter().all(|p| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    })
}

fn tally_run(stats: &mut GitHubRepoStats, run: &GitHubRawRun) {
    match run.status.as_str() {
        "in_progress" | "queued" | "waiting" => {
            stats.in_progress += 1;
            return;
        }
        _ => {}
    }
    match run.conclusion.as_deref() {
        Some("success") => stats.success += 1,
        Some("failure") | Some("timed_out") | Some("startup_failure") => stats.failure += 1,
        _ => stats.other += 1,
    }
}

async fn fetch_github_actions(
    token: &str,
    repos: &[String],
) -> Result<GitHubActionsSummary, String> {
    if repos.is_empty() {
        return Err("GITHUB_REPOS env boş — en az 1 repo gerek".to_string());
    }
    for r in repos {
        if !valid_repo_slug(r) {
            return Err(format!("Repo slug geçersiz: {r} (owner/name alfanümerik + _-. izinli)"));
        }
    }

    let client = reqwest::Client::new();
    let mut all_repos: Vec<GitHubRepoStats> = Vec::with_capacity(repos.len());
    let mut all_runs: Vec<GitHubRun> = Vec::new();

    for repo in repos {
        let url = format!("https://api.github.com/repos/{repo}/actions/runs?per_page=25");
        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "aoe-fork-widgets/1.0")
            .send()
            .await
            .map_err(|e| format!("GitHub request error ({repo}): {e}"))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| format!("GitHub body read error ({repo}): {e}"))?;

        if !status.is_success() {
            return Err(format!("GitHub API {repo} {status}: {text}"));
        }

        let parsed: GitHubWorkflowRunsResp = serde_json::from_str(&text)
            .map_err(|e| format!("GitHub JSON parse error ({repo}): {e}"))?;

        let mut stats = GitHubRepoStats {
            repo: repo.clone(),
            success: 0,
            failure: 0,
            in_progress: 0,
            other: 0,
        };
        for run in &parsed.workflow_runs {
            tally_run(&mut stats, run);
        }
        all_repos.push(stats);

        for r in parsed.workflow_runs {
            all_runs.push(GitHubRun {
                id: r.id,
                repo: repo.clone(),
                name: r.name.unwrap_or_default(),
                status: r.status,
                conclusion: r.conclusion,
                html_url: r.html_url,
                head_branch: r.head_branch.unwrap_or_default(),
                event: r.event,
                run_started_at: r.run_started_at.unwrap_or_default(),
                updated_at: r.updated_at,
            });
        }
    }

    // Cross-repo DESC by updated_at
    all_runs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    all_runs.truncate(5);

    Ok(GitHubActionsSummary {
        repos: all_repos,
        recent: all_runs,
        fetched_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub async fn get_github_actions_summary(
    State(state): State<Arc<AppState>>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let token = env::var("GITHUB_TOKEN").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "GITHUB_TOKEN env eksik".to_string(),
        )
    })?;

    let repos_env = env::var("GITHUB_REPOS").unwrap_or_default();
    let repos = parse_repos(&repos_env);
    if repos.is_empty() {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "GITHUB_REPOS env eksik (owner/name virgülle ayrılmış)".to_string(),
        ));
    }

    if let Some(cached) = state.widget_cache.get_gh() {
        return Ok(with_cache_header(cached, true));
    }

    match fetch_github_actions(&token, &repos).await {
        Ok(data) => {
            state.widget_cache.set_gh(data.clone());
            Ok(with_cache_header(data, false))
        }
        Err(msg) => Err((StatusCode::BAD_GATEWAY, msg)),
    }
}

// ─── Vercel ────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct VercelStateCounts {
    pub ready: usize,
    pub error: usize,
    pub building: usize,
    pub queued: usize,
    pub canceled: usize,
    pub other: usize,
}

#[derive(Clone, Serialize)]
pub struct VercelDeploymentMeta {
    pub branch: Option<String>,
    pub commit_sha: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct VercelDeployment {
    pub uid: String,
    pub name: String,
    pub url: String,
    pub state: String,
    pub target: Option<String>,
    pub created_at: i64,
    pub source: Option<String>,
    pub meta: VercelDeploymentMeta,
}

#[derive(Clone, Serialize)]
pub struct VercelSummary {
    pub counts: VercelStateCounts,
    pub recent: Vec<VercelDeployment>,
    pub fetched_at: String,
}

#[derive(Deserialize)]
struct VercelRawMeta {
    #[serde(rename = "githubCommitRef")]
    github_commit_ref: Option<String>,
    #[serde(rename = "githubCommitSha")]
    github_commit_sha: Option<String>,
}

#[derive(Deserialize)]
struct VercelRawDeployment {
    uid: String,
    name: String,
    url: String,
    state: String,
    target: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: i64,
    source: Option<String>,
    meta: Option<VercelRawMeta>,
}

#[derive(Deserialize)]
struct VercelDeploymentsResp {
    deployments: Vec<VercelRawDeployment>,
}

fn tally_state(counts: &mut VercelStateCounts, state: &str) {
    match state {
        "READY" => counts.ready += 1,
        "ERROR" => counts.error += 1,
        "BUILDING" | "INITIALIZING" => counts.building += 1,
        "QUEUED" => counts.queued += 1,
        "CANCELED" => counts.canceled += 1,
        _ => counts.other += 1,
    }
}

async fn fetch_vercel(
    token: &str,
    project_id: &str,
    team_id: Option<&str>,
) -> Result<VercelSummary, String> {
    if project_id.is_empty() {
        return Err("VERCEL_PROJECT_ID env eksik".to_string());
    }

    let mut url = format!(
        "https://api.vercel.com/v6/deployments?projectId={}&limit=25",
        urlencode_minimal(project_id)
    );
    if let Some(tid) = team_id {
        if !tid.is_empty() {
            url.push_str(&format!("&teamId={}", urlencode_minimal(tid)));
        }
    }

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| format!("Vercel request error: {e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Vercel body read error: {e}"))?;

    if !status.is_success() {
        return Err(format!("Vercel API {status}: {text}"));
    }

    let parsed: VercelDeploymentsResp = serde_json::from_str(&text)
        .map_err(|e| format!("Vercel JSON parse error: {e}"))?;

    let mut counts = VercelStateCounts {
        ready: 0,
        error: 0,
        building: 0,
        queued: 0,
        canceled: 0,
        other: 0,
    };
    for d in &parsed.deployments {
        tally_state(&mut counts, &d.state);
    }

    let recent: Vec<VercelDeployment> = parsed
        .deployments
        .into_iter()
        .take(5)
        .map(|d| {
            let (branch, commit_sha) = match d.meta {
                Some(m) => (m.github_commit_ref, m.github_commit_sha),
                None => (None, None),
            };
            VercelDeployment {
                uid: d.uid,
                name: d.name,
                url: d.url,
                state: d.state,
                target: d.target,
                created_at: d.created_at,
                source: d.source,
                meta: VercelDeploymentMeta { branch, commit_sha },
            }
        })
        .collect();

    Ok(VercelSummary {
        counts,
        recent,
        fetched_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Minimal URL-safe encoding for Vercel project/team ids (alphanumeric + `_-`).
/// Reject anything else (defense-in-depth — these come from env, but cheap).
fn urlencode_minimal(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect()
}

pub async fn get_vercel_summary(
    State(state): State<Arc<AppState>>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let token = env::var("VERCEL_TOKEN").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "VERCEL_TOKEN env eksik".to_string(),
        )
    })?;

    let project_id = env::var("VERCEL_PROJECT_ID").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "VERCEL_PROJECT_ID env eksik".to_string(),
        )
    })?;

    let team_id_owned = env::var("VERCEL_TEAM_ID").ok();
    let team_id = team_id_owned.as_deref();

    if let Some(cached) = state.widget_cache.get_vercel() {
        return Ok(with_cache_header(cached, true));
    }

    match fetch_vercel(&token, &project_id, team_id).await {
        Ok(data) => {
            state.widget_cache.set_vercel(data.clone());
            Ok(with_cache_header(data, false))
        }
        Err(msg) => Err((StatusCode::BAD_GATEWAY, msg)),
    }
}

// ─── Netdata ───────────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct NetdataChart {
    pub id: &'static str,
    pub url: String,
}

#[derive(Clone, Serialize)]
pub struct NetdataSummary {
    pub reachable: bool,
    pub version: String,
    pub host: String,
    pub charts: Vec<NetdataChart>,
    pub iframe_base: String,
    pub fetched_at: String,
}

#[derive(Deserialize)]
struct NetdataInfoResp {
    version: Option<String>,
    #[serde(rename = "mirrored_hosts")]
    mirrored_hosts: Option<Vec<String>>,
    hostname: Option<String>,
}

const NETDATA_CHARTS: &[&str] = &["system.cpu", "system.ram", "system.load", "system.io"];

async fn fetch_netdata(base_url: &str) -> Result<NetdataSummary, String> {
    let base = base_url.trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err("NETDATA_BASE_URL env boş".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| format!("Netdata client init error: {e}"))?;

    let resp = client
        .get(format!("{base}/api/v1/info"))
        .send()
        .await
        .map_err(|e| format!("Netdata request error: {e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Netdata body read error: {e}"))?;

    if !status.is_success() {
        return Err(format!("Netdata API {status}: {text}"));
    }

    let info: NetdataInfoResp = serde_json::from_str(&text)
        .map_err(|e| format!("Netdata JSON parse error: {e}"))?;

    let host = info
        .hostname
        .or_else(|| info.mirrored_hosts.and_then(|h| h.into_iter().next()))
        .unwrap_or_else(|| "localhost".to_string());

    let charts: Vec<NetdataChart> = NETDATA_CHARTS
        .iter()
        .map(|id| NetdataChart {
            id,
            url: format!("{base}/host/{host}/{id}"),
        })
        .collect();

    Ok(NetdataSummary {
        reachable: true,
        version: info.version.unwrap_or_else(|| "unknown".to_string()),
        host,
        charts,
        iframe_base: base,
        fetched_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub async fn get_netdata_summary(
    State(state): State<Arc<AppState>>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let base_url = env::var("NETDATA_BASE_URL").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "NETDATA_BASE_URL env eksik (örn http://localhost:19999)".to_string(),
        )
    })?;

    if let Some(cached) = state.widget_cache.get_netdata() {
        return Ok(with_cache_header(cached, true));
    }

    match fetch_netdata(&base_url).await {
        Ok(data) => {
            state.widget_cache.set_netdata(data.clone());
            Ok(with_cache_header(data, false))
        }
        Err(msg) => Err((StatusCode::BAD_GATEWAY, msg)),
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_repos_handles_whitespace_and_filters() {
        let v = parse_repos(" furkangurr/ajan-sistemi ,  furkangurr/avukatadanis-online , , bad-no-slash ,  ");
        assert_eq!(
            v,
            vec![
                "furkangurr/ajan-sistemi".to_string(),
                "furkangurr/avukatadanis-online".to_string(),
            ]
        );
    }

    #[test]
    fn valid_repo_slug_accepts_owner_name() {
        assert!(valid_repo_slug("furkangurr/ajan-sistemi"));
        assert!(valid_repo_slug("foo/bar.baz"));
        assert!(valid_repo_slug("a/b_c"));
        assert!(!valid_repo_slug("/x"));
        assert!(!valid_repo_slug("x/"));
        assert!(!valid_repo_slug("x"));
        assert!(!valid_repo_slug("x/y/z"));
        assert!(!valid_repo_slug("x/y;z"));
    }

    #[test]
    fn valid_sentry_slug_rejects_specials() {
        assert!(valid_sentry_slug("furkangurr"));
        assert!(valid_sentry_slug("avukatadanis-online"));
        assert!(valid_sentry_slug("org.with.dots"));
        assert!(!valid_sentry_slug(""));
        assert!(!valid_sentry_slug("has space"));
        assert!(!valid_sentry_slug("inj;ect"));
    }

    #[test]
    fn urlencode_minimal_strips_specials() {
        assert_eq!(urlencode_minimal("prj_abc-123"), "prj_abc-123");
        assert_eq!(urlencode_minimal("prj;bad"), "prjbad");
        assert_eq!(urlencode_minimal(""), "");
    }

    #[test]
    fn tally_run_buckets_correctly() {
        let mut s = GitHubRepoStats {
            repo: "x/y".into(),
            success: 0,
            failure: 0,
            in_progress: 0,
            other: 0,
        };
        tally_run(
            &mut s,
            &GitHubRawRun {
                id: 1,
                name: None,
                status: "completed".into(),
                conclusion: Some("success".into()),
                html_url: "".into(),
                head_branch: None,
                event: "push".into(),
                run_started_at: None,
                updated_at: "".into(),
            },
        );
        tally_run(
            &mut s,
            &GitHubRawRun {
                id: 2,
                name: None,
                status: "in_progress".into(),
                conclusion: None,
                html_url: "".into(),
                head_branch: None,
                event: "push".into(),
                run_started_at: None,
                updated_at: "".into(),
            },
        );
        tally_run(
            &mut s,
            &GitHubRawRun {
                id: 3,
                name: None,
                status: "completed".into(),
                conclusion: Some("failure".into()),
                html_url: "".into(),
                head_branch: None,
                event: "push".into(),
                run_started_at: None,
                updated_at: "".into(),
            },
        );
        assert_eq!(s.success, 1);
        assert_eq!(s.in_progress, 1);
        assert_eq!(s.failure, 1);
        assert_eq!(s.other, 0);
    }

    #[test]
    fn tally_state_buckets_correctly() {
        let mut c = VercelStateCounts {
            ready: 0,
            error: 0,
            building: 0,
            queued: 0,
            canceled: 0,
            other: 0,
        };
        tally_state(&mut c, "READY");
        tally_state(&mut c, "ERROR");
        tally_state(&mut c, "BUILDING");
        tally_state(&mut c, "INITIALIZING");
        tally_state(&mut c, "QUEUED");
        tally_state(&mut c, "CANCELED");
        tally_state(&mut c, "WEIRD");
        assert_eq!(c.ready, 1);
        assert_eq!(c.error, 1);
        assert_eq!(c.building, 2);
        assert_eq!(c.queued, 1);
        assert_eq!(c.canceled, 1);
        assert_eq!(c.other, 1);
    }
}
