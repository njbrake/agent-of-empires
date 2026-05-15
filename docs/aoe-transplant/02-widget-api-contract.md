# 02 — Widget API Contract (Code-1 → Code-2)

> **Status:** Draft v1 (Code-1 dondurucu, 2026-05-15T20:55Z).  
> **Yazar:** Code-1 Rust core scope (FUR-3957 transplant Adım 1+2).  
> **Tüketici:** Code-2 React PWA scope (Adım 3+4 widget component port).  
> **Parent:** FUR-3957 (AoE fork uyarlama, Furkan canon 2026-05-15T13:30Z revize).  
> **Prequel:** FUR-3965 Sub-B Next.js MVP (Done, lokum kıvamı) — 5 widget veri kaynağı pattern bu kontrata port edildi.

## Genel sözleşme

Tüm widget endpoint'leri ortak pattern paylaşır:

- **Base path:** `/api/widgets/<service>/summary` (GET)
- **Auth:** Axum server zaten Tailscale arkasında — endpoint-level auth yok (network-layer trust)
- **Cache:** İç süreç 60sn TTL per endpoint (`WidgetCache` struct, `src/server/api/widgets.rs`). Tek instance — horizontally scalable değil (multi-pod deployment ileride Redis swap)
- **Cache header:** `x-cache: HIT` (cache döndü) veya `x-cache: MISS` (upstream fetch + cache write)
- **Content-Type:** `application/json; charset=utf-8` (axum `Json<T>` default)
- **JSON convention:** **`snake_case`** field names (Rust serde default, `#[serde(rename_all = "camelCase")]` uygulanmaz — Code-2 TS client'ı kendi typing'inde snake_case veya wrapper kullanabilir)
- **Timestamp:** ISO 8601 (`chrono::Utc::now().to_rfc3339()`) — `fetched_at` field her response'ta

## Error response sözleşmesi

| Status | Tetik | Body shape |
|---|---|---|
| `500 INTERNAL_SERVER_ERROR` | Required env var(lar) eksik veya boş | Plain text: `"<ENV_VAR> env eksik"` veya kompozit hata mesajı |
| `502 BAD_GATEWAY` | Upstream API fail (non-2xx HTTP, GraphQL errors, JSON parse fail, network) | Plain text: `"<service> API <status>: <body excerpt>"` veya `"<service> request error: <io error>"` |
| `200 OK` | Başarı + cache write | JSON body (aşağıdaki per-endpoint shape) |

500/502 cache'e yazılmaz — sonraki istek tekrar upstream'i dener.

**Important:** Axum handler `Result<impl IntoResponse, (StatusCode, String)>` döner. Plain text body dolayı Code-2 fetch error path'i: `if (!res.ok) { const errText = await res.text(); ... }`.

## Common types

```rust
// src/server/api/widgets.rs
pub struct WidgetCache {
    linear: Mutex<Option<Cached<LinearSummary>>>,
    sentry: Mutex<Option<Cached<SentrySummary>>>,
    github_actions: Mutex<Option<Cached<GitHubActionsSummary>>>,
    vercel: Mutex<Option<Cached<VercelSummary>>>,
    netdata: Mutex<Option<Cached<NetdataSummary>>>,
}
```

---

## 1. `GET /api/widgets/linear/summary`

**Status:** ✅ LIVE (existing, FUR-3957 Sub-C port — ajan-sistemi#521 Next.js karşılığı).

**Env (zorunlu):**
- `LINEAR_API_KEY` — Linear Restricted veya Personal API key

**Response 200:**

```json
{
  "in_progress": { "count": 24, "has_more": false },
  "backlog":     { "count": 187, "has_more": false },
  "done7d":      { "count": 250, "has_more": true },
  "recent": [
    {
      "identifier": "FUR-3966",
      "title": "[FUR-3957 Sub-C] Linear+Sentry widget endpoints",
      "state": "Done",
      "url": "https://linear.app/dilsihirdir/issue/FUR-3966",
      "updated_at": "2026-05-15T19:34:33Z"
    }
  ],
  "fetched_at": "2026-05-15T20:14:23.5Z"
}
```

**Notlar:** `has_more=true` → 250+ issue saturate (Linear `first: 250` limit). Team ID sabit (`5669ce66-…204f` avukatadanış). Done7d filter: `completedAt >= now() - 7 days`.

---

## 2. `GET /api/widgets/sentry/summary`

**Status:** ✅ LIVE (existing, FUR-3957 Sub-C port — Next.js karşılığı).

**Env (üçü de zorunlu):**
- `SENTRY_AUTH_TOKEN`
- `SENTRY_ORG` (örn `furkangurr`)
- `SENTRY_PROJECT_SLUG` (örn `avukatadanis-online`)

Slug/org alfanümerik + `-_.` izinli; aksi 500.

**Response 200:**

```json
{
  "total_issues": 12,
  "unresolved": 8,
  "recent": [
    {
      "id": "12345",
      "short_id": "AVUKATADANIS-ONLINE-42",
      "title": "TypeError: Cannot read property 'foo' of undefined",
      "culprit": "src/foo.ts",
      "count": "42",
      "permalink": "https://furkangurr.sentry.io/issues/12345/",
      "last_seen": "2026-05-15T20:10:00Z"
    }
  ],
  "fetched_at": "2026-05-15T20:14:23.5Z"
}
```

**Notlar:** `statsPeriod=24h`, `limit=25`, `sort=date`, `query=is:unresolved`. `unresolved` field = post-filter sayı. `recent` = unresolved'den slice(0, 5).

---

## 3. `GET /api/widgets/github-actions/summary` (Code-1 NEW)

**Status:** 📋 Pending implementation (FUR-3957 transplant Adım 2, this contract).

**Env (ikisi de zorunlu):**
- `GITHUB_TOKEN` — PAT veya fine-grained token, `actions:read` scope
- `GITHUB_REPOS` — comma-separated `owner/name,owner/name` (en az 1, parse sonrası boş ise 500)

**Response 200:**

```json
{
  "repos": [
    {
      "repo": "furkangurr/ajan-sistemi",
      "success": 18,
      "failure": 2,
      "in_progress": 1,
      "other": 4
    },
    {
      "repo": "furkangurr/avukatadanis-online",
      "success": 22,
      "failure": 0,
      "in_progress": 0,
      "other": 3
    }
  ],
  "recent": [
    {
      "id": 25937797222,
      "repo": "furkangurr/ajan-sistemi",
      "name": "canon-doc-verify",
      "status": "completed",
      "conclusion": "success",
      "html_url": "https://github.com/furkangurr/ajan-sistemi/actions/runs/25937797222",
      "head_branch": "main",
      "event": "push",
      "run_started_at": "2026-05-15T19:41:53Z",
      "updated_at": "2026-05-15T19:42:07Z"
    }
  ],
  "fetched_at": "2026-05-15T20:14:23.5Z"
}
```

**Notlar:**
- `repos`: per-repo workflow runs son 25 (`per_page=25`), status tally
  - `success`: `conclusion === "success"`
  - `failure`: `conclusion in [failure, timed_out, startup_failure]`
  - `in_progress`: `status in [in_progress, queued, waiting]`
  - `other`: skipped / cancelled / neutral / null
- `recent`: cross-repo `updated_at` DESC, slice(0, 5)
- Multi-repo `tokio::join!` paralel fetch; tek repo fail → 502 tüm endpoint (orijinal Next.js pattern aynen)

---

## 4. `GET /api/widgets/vercel/summary` (Code-1 NEW)

**Status:** 📋 Pending implementation.

**Env (token + project_id zorunlu, team_id opsiyonel):**
- `VERCEL_TOKEN`
- `VERCEL_PROJECT_ID` (örn `prj_xxxxx`)
- `VERCEL_TEAM_ID` (opsiyonel, team scope token için)

**Response 200:**

```json
{
  "counts": {
    "ready": 18,
    "error": 2,
    "building": 1,
    "queued": 0,
    "canceled": 3,
    "other": 1
  },
  "recent": [
    {
      "uid": "dpl_xxxx",
      "name": "avukatadanis-online",
      "url": "avukatadanis-online-git-main-furkangurr.vercel.app",
      "state": "READY",
      "target": "production",
      "created_at": 1715789600000,
      "source": "git",
      "meta": {
        "branch": "main",
        "commit_sha": "abc123def"
      }
    }
  ],
  "fetched_at": "2026-05-15T20:14:23.5Z"
}
```

**Notlar:**
- Vercel v6 `/deployments?projectId=…&limit=25` (+ optional `teamId`)
- `counts` bucket: `BUILDING + INITIALIZING → building`, diğer state'ler kendi bucket'larına
- `created_at` epoch ms (Vercel API native int) — Code-2 `new Date(createdAt)` ile parse
- `recent` slice(0, 5), Vercel API zaten DESC döner
- `meta.branch` / `meta.commit_sha` — Vercel `meta.githubCommitRef` / `meta.githubCommitSha` field'larından normalize (snake_case JSON)

---

## 5. `GET /api/widgets/netdata/summary` (Code-1 NEW, simplified scope)

**Status:** 📋 Pending implementation.

**Scope (Code-1 dropdown):** Backend reachability check + chart URL listesi. Frontend iframe Netdata UI'sini direkt render eder — backend sadece available/down sinyali + canonical URL'ler.

**Env (zorunlu):**
- `NETDATA_BASE_URL` (örn `http://localhost:19999`) — Netdata server taban URL

**Response 200:**

```json
{
  "reachable": true,
  "version": "v1.42.0",
  "host": "avk-suite",
  "charts": [
    { "id": "system.cpu", "url": "http://localhost:19999/host/avk-suite/system.cpu" },
    { "id": "system.ram", "url": "http://localhost:19999/host/avk-suite/system.ram" },
    { "id": "system.load", "url": "http://localhost:19999/host/avk-suite/system.load" },
    { "id": "system.io", "url": "http://localhost:19999/host/avk-suite/system.io" }
  ],
  "iframe_base": "http://localhost:19999",
  "fetched_at": "2026-05-15T20:14:23.5Z"
}
```

**Notlar:**
- Reachability check: `GET <NETDATA_BASE_URL>/api/v1/info` — başarılı 200 ise `reachable=true`
- `version` + `host` Netdata `info` response'tan
- `charts`: hardcoded canonical 4 chart (CPU/RAM/load/io) — Frontend iframe src'lerinde kullanır
- Upstream fail → 502 (Netdata down)
- Env eksik → 500
- Netdata public iframe'leri bilinçli olarak destekliyor (`NETDATA_API_INFO_NO_AUTH` default)

---

## Cache invalidation

Yok — 60sn TTL dolmasını bekle veya `aoe serve` restart. Manuel flush endpoint planlanmıyor.

## Concurrency

`WidgetCache` `Mutex<Option<Cached<T>>>` — kısa kritik bölge (clone + return). Multi-request paralel "thundering herd" potansiyeli var: 2 istek cache miss aynı anda → 2 upstream call. Kabul edilebilir (60sn pencere geniş, Linear/Sentry rate limit yumuşak). Mitigation gerekirse `tokio::sync::Mutex` + double-check pattern.

## Smoke test

Backend hazır olunca `tools/widget-smoke.sh` script ile verify edilecek (Code-2 tüketim öncesi sanity):

```bash
BASE=http://localhost:4096   # aoe serve default
for ep in linear sentry github-actions vercel netdata; do
    curl -sS -D - -o /dev/null "$BASE/api/widgets/$ep/summary" | head -5
done
```

Env eksikse 500, set ise 200 + `x-cache: MISS` (sonra HIT) bekleniyor.

## Versioning

Bu doc v1. Breaking change olursa v2 ayrı dosya (`02-widget-api-contract-v2.md`) + endpoint path `/api/widgets/v2/...` (gerekirse). Şu an gerekmiyor.

## Atomic-Lock

- **Code-1 sole:** `src/server/api/widgets.rs` + `src/server/api/mod.rs` + `src/server/mod.rs` route registration + `WidgetCache` struct extend
- **Code-2 sole:** `web/src/lib/integrations/{linear,sentry,github,vercel,netdata}.ts` (TS client) + `web/src/components/widgets/*` (UI component) + `Dashboard.tsx` grid entegrasyonu
- **Cross-cutting (this doc):** Code-1 dondurucu, Code-2 tüketici. Breaking değişiklik gerekirse Code-1 önce bu doc'u günceller, Code-2 ack verir.

Refs: FUR-3957, FUR-3965 (prequel), parent FUR-3954.
