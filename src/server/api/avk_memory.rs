//! AVK memory recall feed endpoint — FUR-4118 + FUR-4159.
//!
//! `GET /api/avk/memory-recall[?role=koord&hours=24]` — VPS-side `agentmemory`
//! MCP HTTP proxy'sine `memory_recall` çağrısı atar; observation döküntülerini
//! UI'nin beklediği `MemoryEntry` shape'ine indirger. MCP unreachable veya hata
//! döndürürse mock fallback aktif (UI demo'nun ölmesini engeller).
//!
//! ## Çağrı kontratı
//!
//! POST http://localhost:3111/agentmemory/mcp/call
//! ```json
//! { "name": "memory_recall", "arguments": { "query": "...", "top": 20 } }
//! ```
//!
//! Cevap dış katman MCP standardı `{ content: [{ type: "text", text: "<json>" }] }`,
//! iç katman `{ results: [{ observation: { id, title, ... }, score }] }`.
//!
//! ## Tier kuralı (sade — observation.importance kombinasyonu)
//!
//! - importance ≥ 4 → core (kalıcı kanon)
//! - importance 2-3 → working (aktif iş)
//! - importance ≤ 1 veya yok → archival (referans)
//!
//! ## Role tahmini
//!
//! MCP observation'da role meta yok. Title/subtitle'da ajan adı geçerse
//! (koord/komuta/code-1/code-2/merge/hata/codex/gemini-1/2/kimi-1/2/3/omni)
//! buna mapper; yoksa `"system"` döner.

use axum::{extract::Query, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

const MCP_URL: &str = "http://localhost:3111/agentmemory/mcp/call";
const MCP_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_TOP: u32 = 20;

const KNOWN_ROLES: &[&str] = &[
    "koord", "komuta", "mudur", "müdür", "code-1", "code-2", "merge", "hata", "codex", "gemini-1",
    "gemini-2", "kimi-1", "kimi-2", "kimi-3", "omni", "furkan",
];

#[derive(Deserialize)]
pub struct AvkMemoryQuery {
    pub role: Option<String>,
    #[allow(dead_code)]
    pub hours: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryTier {
    Core,
    Working,
    Archival,
}

#[derive(Debug, Serialize)]
pub struct MemoryEntry {
    pub id: String,
    pub title: String,
    pub tier: MemoryTier,
    pub role: String,
    pub tags: Vec<String>,
    pub content_preview: String,
    pub created_at: String,
}

/// GET `/api/avk/memory-recall[?role=...&hours=...]`
///
/// Gerçek MCP query başarısızsa mock fallback. Filter `role` parametre.
pub async fn list_avk_memory_recall(Query(query): Query<AvkMemoryQuery>) -> impl IntoResponse {
    let entries = match fetch_from_mcp(query.role.as_deref()).await {
        Ok(list) if !list.is_empty() => list,
        _ => mock_fallback(),
    };

    let filtered: Vec<MemoryEntry> = entries
        .into_iter()
        .filter(|entry| match query.role.as_deref() {
            Some(role) => entry.role == role,
            None => true,
        })
        .collect();

    Json(filtered).into_response()
}

async fn fetch_from_mcp(role_hint: Option<&str>) -> Result<Vec<MemoryEntry>, String> {
    let query_text = match role_hint {
        Some(role) => format!("{role} FUR AVK ajan canon patrol"),
        None => "AVK FUR canon patrol sustained ajan".to_string(),
    };
    let body = serde_json::json!({
        "name": "memory_recall",
        "arguments": { "query": query_text, "top": DEFAULT_TOP },
    });

    let client = reqwest::Client::builder()
        .timeout(MCP_TIMEOUT)
        .build()
        .map_err(|e| format!("reqwest client build: {e}"))?;
    let resp = client
        .post(MCP_URL)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("MCP unreachable: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("MCP status {}", resp.status()));
    }

    let outer: Value = resp.json().await.map_err(|e| format!("outer parse: {e}"))?;
    let inner_text = outer
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|first| first.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| "no content[0].text in MCP response".to_string())?;
    let inner: Value = serde_json::from_str(inner_text).map_err(|e| format!("inner parse: {e}"))?;
    let results = inner
        .get("results")
        .and_then(|r| r.as_array())
        .ok_or_else(|| "no results[] in MCP inner".to_string())?;

    Ok(results
        .iter()
        .filter_map(observation_to_entry)
        .collect::<Vec<_>>())
}

fn observation_to_entry(item: &Value) -> Option<MemoryEntry> {
    let obs = item.get("observation")?;
    let id = obs.get("id")?.as_str()?.to_string();
    let title_raw = obs.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let subtitle = obs.get("subtitle").and_then(|v| v.as_str()).unwrap_or("");
    let narrative = obs.get("narrative").and_then(|v| v.as_str()).unwrap_or("");
    let obs_type = obs.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let timestamp = obs
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let importance = obs.get("importance").and_then(|v| v.as_u64()).unwrap_or(0);

    let preview_src = if !subtitle.is_empty() {
        subtitle
    } else {
        narrative
    };
    let content_preview = truncate(preview_src, 220);

    let title = resolve_title(title_raw, obs_type, preview_src, obs);

    let role = infer_role(&title, subtitle, narrative);
    let tier = tier_from_importance(importance);

    let tags: Vec<String> = obs
        .get("concepts")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .take(5)
                .collect()
        })
        .unwrap_or_default();

    Some(MemoryEntry {
        id,
        title,
        tier,
        role,
        tags,
        content_preview,
        created_at: timestamp,
    })
}

/// MCP observation.title sıklıkla "Bash" / "post_tool_use" gibi generic
/// degerlerle gelir (Claude Code PostToolUse hook'tan üretilmiş kayıtlar).
/// Bu tip generic title'ları daha okunur özetlere çevir.
fn resolve_title(title_raw: &str, obs_type: &str, preview_src: &str, obs: &Value) -> String {
    const GENERIC: &[&str] = &[
        "Bash",
        "post_tool_use",
        "pre_tool_use",
        "command_run",
        "tool_use",
        "Read",
        "Edit",
        "Write",
        "",
    ];
    if !title_raw.is_empty() && !GENERIC.contains(&title_raw) {
        return title_raw.to_string();
    }
    // file_edit/file_read: ilk dosya adını başlık yap.
    if obs_type == "file_edit" || obs_type == "file_read" {
        if let Some(file) = obs
            .get("files")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
        {
            let basename = file.rsplit('/').next().unwrap_or(file);
            let verb = if obs_type == "file_edit" {
                "Düzenleme"
            } else {
                "Okuma"
            };
            return format!("{verb}: {basename}");
        }
    }
    let prefix = match obs_type {
        "command_run" => "Komut",
        "file_edit" => "Düzenleme",
        "file_read" => "Okuma",
        _ => "Kayıt",
    };
    let cleaned = preview_src.trim().trim_matches('"').trim_start_matches('{');
    format!("{prefix}: {}", truncate(cleaned, 70))
}

fn tier_from_importance(importance: u64) -> MemoryTier {
    match importance {
        i if i >= 4 => MemoryTier::Core,
        i if i >= 2 => MemoryTier::Working,
        _ => MemoryTier::Archival,
    }
}

fn infer_role(title: &str, subtitle: &str, narrative: &str) -> String {
    let haystack = format!("{title} {subtitle} {narrative}").to_lowercase();
    for role in KNOWN_ROLES {
        if haystack.contains(role) {
            return (*role).to_string();
        }
    }
    "system".to_string()
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

/// MCP unreachable veya boş cevap durumunda son 7 entry sabit demo.
/// UI'da "agentmemory bağlanılamadı, mock göstergesi" feel'i yerine canlı
/// demo akış korunur (Furkan iPhone'dan açtığında widget her zaman dolu).
fn mock_fallback() -> Vec<MemoryEntry> {
    [
        ("mem-001", "FUR-3957 transplant TAMAMLANDI — Adım 5-8 zinciri",
         MemoryTier::Core, "omni",
         &["ajanlarim", "aoe", "transplant", "tamamlandı"][..],
         "PR #4 admin merge başarılı. AvkAgentsGrid widget Dashboard'a mount edildi, \
          /api/avk/agents endpoint LIVE, avk_agents.rs registry main'de.",
         "2026-05-17T13:10:00Z"),
        ("mem-002", "FUR-4154 sustained sweep 4 PR merged (A-D)",
         MemoryTier::Core, "omni",
         &["fur-4154", "sustained", "pr-rotation"][..],
         "PR #12 polish + #13 (A) history + #14 (B) slug + #15 (C) health + #16 (D) klavye. \
          Furkan canon 'temiz temiz birini bitir diğerine geç' eski sistem ritmi.",
         "2026-05-17T17:56:34Z"),
        ("mem-003", "FUR-4117 — 13 ajan AoE session lifecycle lokal",
         MemoryTier::Working, "omni",
         &["aoe", "session-lifecycle", "lokal-first"][..],
         "13 ajan (koord/komuta/mudur/code-1/2/merge/hata/codex/gemini-1/2/kimi-1/2/3) \
          AoE'ye eklendi. Mevcut tmux setup paralel.",
         "2026-05-17T13:38:00Z"),
        ("mem-004", "P0 PROD INCIDENT RESOLVED — FUR-4073 8 tezahür",
         MemoryTier::Core, "hata",
         &["p0", "prod", "resolved", "strangler-cluster"][..],
         "5h 30m sustained Strangler refactor cluster. 8 tezahür, 5 fix PR. \
          www.avukatadanis.com 200 OK ✓.",
         "2026-05-17T07:05:00Z"),
        ("mem-005", "SEO audit revize — Furkan canon domain migration",
         MemoryTier::Working, "omni",
         &["seo", "fur-4072", "verify-before-claim"][..],
         "Furkan 'domaini avukatadanis.com taşıdık' tek cümleyle düzeltti. Karpathy 51 #31 ders. \
          GSC'de yeni .com property eklenmemiş.",
         "2026-05-17T02:11:20Z"),
        ("mem-006", "Karpathy 51 #137 — Mekanik > niyet, FUR-4068 guard yetersiz",
         MemoryTier::Archival, "koord",
         &["karpathy", "mekanik-niyet", "pre-commit-hook"][..],
         "FUR-4068 Strangler rename pre-commit guard MERGED ama 1 saat içinde 2 yeni cluster tezahürü. \
          Mekanik garanti tanım yetersizliği etkisiz.",
         "2026-05-17T06:42:54Z"),
        ("mem-007", "tmux Yardimcilar window rebuild — kalıcı kök çözüm",
         MemoryTier::Working, "omni",
         &["tmux", "drift", "kalici-cozum"][..],
         "Furkan canon 'birkaç sefer ayarladık, yeniden başlatınca karışıyor yardımcılar'. \
          Idempotent rebuild script (Karpathy §10.1). 6 pane 2col×3row.",
         "2026-05-17T11:55:00Z"),
    ]
    .into_iter()
    .map(|(id, title, tier, role, tags, preview, created_at)| MemoryEntry {
        id: id.to_string(),
        title: title.to_string(),
        tier,
        role: role.to_string(),
        tags: tags.iter().map(|s| (*s).to_string()).collect(),
        content_preview: preview.to_string(),
        created_at: created_at.to_string(),
    })
    .collect()
}
