//! AVK workflow agent registry.
//!
//! FUR-3957 transplant Adım 5 — bizim 13 multi-agent workflow ajan kaydı.
//! Paralel yapı: `src/agents.rs` AoE upstream CLI binary registry (Claude/
//! Cursor/Codex CLI tespiti); bu dosya **bizim sistem rolleri** (Koord,
//! Komuta, Müdür, Code-1/2, Hata, Merge, Gemini-1/2, Kimi-1/2/3, Codex).
//!
//! UI tarafı web/src/lib/agents-avk.ts (Code-2 ayrı PR scope) bu listeyi
//! mirrors — slug eşleşmesi single source of truth burada.
//!
//! ## Kategori
//!
//! - **Director**: yönetim (Koord/Komuta/Müdür)
//! - **Senior**: kıdemli kod + ops (Code-1/2, Hata, Merge)
//! - **Worker**: research + paralel slot (Gemini-1/2, Kimi-1/2/3, Codex)
//!
//! ## Atomic-Lock
//!
//! Atomic-Lock: FUR-3957 Adım 5 (Code-2 sole) — Koord karar 02:05Z
//! atomic-lock revize. Code-1 paralel F5.4 caller migration (avukatadanis-
//! online src/app/api/*) sustained.

/// AVK workflow ajan kategorisi.
///
/// Director-tier ajanları sistemi yönetir (vizyon, dispatch, gateway).
/// Senior-tier ajanları kıdemli iş yapar (kod, audit, merge, CI fix).
/// Worker-tier ajanları paralel slot çalışır (research, code, audit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AvkAgentRole {
    Director,
    Senior,
    Worker,
}

impl AvkAgentRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            AvkAgentRole::Director => "director",
            AvkAgentRole::Senior => "senior",
            AvkAgentRole::Worker => "worker",
        }
    }
}

/// Tek bir AVK workflow ajan tanımı.
///
/// `tmux_target` canlı pane konumu (window:pane-index). Dashboard UI
/// embed iframe veya status query buradan açar. Düzeltme: pane index
/// runtime'da değişebilir (yeni pane ekleme/silme); UI tarafı periyodik
/// `tmux list-panes` ile re-sync etmeli.
#[derive(Debug, serde::Serialize)]
pub struct AvkAgent {
    pub slug: &'static str,
    pub label: &'static str,
    pub role: AvkAgentRole,
    pub tmux_target: &'static str,
}

/// AVK workflow ajan kayıt listesi (13 ajan).
///
/// Sıra: idare window → uretim window → yardimcilar window. Slug stable
/// (UI ve API contract'inde primary key). Label kullanıcıya gösterilen
/// Türkçe ad.
///
/// Karpathy 51 verify-first canon: bu liste `tmux list-panes -a -F
/// '#{window_name}/#{pane_index} #{pane_title}'` çıktısından doğrulandı
/// (2026-05-16T02:25Z TRT, 13/13 pane LIVE ack).
pub const AVK_AGENTS: &[AvkAgent] = &[
    // idare window (yönetim, 4 ajan)
    AvkAgent {
        slug: "koord",
        label: "Koord (Genel Sekreter)",
        role: AvkAgentRole::Director,
        tmux_target: "avk-ofis:idare.1",
    },
    AvkAgent {
        slug: "komuta",
        label: "Komuta Merkezi (Operasyon Müdürü)",
        role: AvkAgentRole::Director,
        tmux_target: "avk-ofis:idare.2",
    },
    AvkAgent {
        slug: "merge",
        label: "Birleştirme Ajanı (PR Bekçi)",
        role: AvkAgentRole::Senior,
        tmux_target: "avk-ofis:idare.3",
    },
    AvkAgent {
        slug: "hata",
        label: "Hata Ajanı (CI Triyaj)",
        role: AvkAgentRole::Senior,
        tmux_target: "avk-ofis:idare.4",
    },
    // uretim window (kıdemli iş, 3 ajan)
    AvkAgent {
        slug: "mudur",
        label: "Müdür (Geçit Süzgeci)",
        role: AvkAgentRole::Director,
        tmux_target: "avk-ofis:uretim.1",
    },
    AvkAgent {
        slug: "code-1",
        label: "Code-1 (Kıdemli Mühendis)",
        role: AvkAgentRole::Senior,
        tmux_target: "avk-ofis:uretim.2",
    },
    AvkAgent {
        slug: "code-2",
        label: "Code-2 (Slot-2 Mühendis)",
        role: AvkAgentRole::Senior,
        tmux_target: "avk-ofis:uretim.3",
    },
    // yardimcilar window (paralel slot, 6 ajan)
    // UI gösterim sırası: Gemini'ler birlikte, sonra Kimi'ler, sonra Codex
    // (Furkan canon 2026-05-17). tmux_target değişmedi — pane index VPS layout.
    AvkAgent {
        slug: "gemini-1",
        label: "Gemini-1 (Araştırmacı)",
        role: AvkAgentRole::Worker,
        tmux_target: "avk-ofis:yardimcilar.1",
    },
    AvkAgent {
        slug: "gemini-2",
        label: "Gemini-2 (Araştırmacı)",
        role: AvkAgentRole::Worker,
        tmux_target: "avk-ofis:yardimcilar.6",
    },
    AvkAgent {
        slug: "kimi-1",
        label: "Kimi-1 (Çoklu Sağlayıcı)",
        role: AvkAgentRole::Worker,
        tmux_target: "avk-ofis:yardimcilar.2",
    },
    AvkAgent {
        slug: "kimi-2",
        label: "Kimi-2 (Çoklu Sağlayıcı)",
        role: AvkAgentRole::Worker,
        tmux_target: "avk-ofis:yardimcilar.3",
    },
    AvkAgent {
        slug: "kimi-3",
        label: "Kimi-3 (Çoklu Sağlayıcı)",
        role: AvkAgentRole::Worker,
        tmux_target: "avk-ofis:yardimcilar.4",
    },
    AvkAgent {
        slug: "codex",
        label: "Codex (Bağımsız Denetçi)",
        role: AvkAgentRole::Worker,
        tmux_target: "avk-ofis:yardimcilar.5",
    },
];

/// Slug ile AVK ajan ara. None döner bilinmeyen slug için.
pub fn find_by_slug(slug: &str) -> Option<&'static AvkAgent> {
    AVK_AGENTS.iter().find(|a| a.slug == slug)
}

/// Rol kategorisi ile ajanları filtrele.
pub fn filter_by_role(role: AvkAgentRole) -> impl Iterator<Item = &'static AvkAgent> {
    AVK_AGENTS.iter().filter(move |a| a.role == role)
}

/// Tier keyword (director/senior/worker/all) AVK ajan slug listesine çevir.
///
/// CLI `aoe send <tier> "<msg>"` (FUR-4120) ve server POST
/// `/api/avk/broadcast` (FUR-4121) ortak kullanır. Bilinmeyen keyword için
/// `None` döner (tekil session send fallback'i çağıran tarafta yapılır).
pub fn resolve_tier_slugs(tier: &str) -> Option<Vec<&'static str>> {
    match tier {
        "all" => Some(AVK_AGENTS.iter().map(|a| a.slug).collect()),
        "director" => Some(
            filter_by_role(AvkAgentRole::Director)
                .map(|a| a.slug)
                .collect(),
        ),
        "senior" => Some(
            filter_by_role(AvkAgentRole::Senior)
                .map(|a| a.slug)
                .collect(),
        ),
        "worker" => Some(
            filter_by_role(AvkAgentRole::Worker)
                .map(|a| a.slug)
                .collect(),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_count_is_thirteen() {
        // tmux 13 pane LIVE ack (2026-05-16T02:25Z TRT).
        assert_eq!(AVK_AGENTS.len(), 13);
    }

    #[test]
    fn all_slugs_unique() {
        let mut slugs: Vec<&str> = AVK_AGENTS.iter().map(|a| a.slug).collect();
        slugs.sort();
        let unique_len = slugs.len();
        slugs.dedup();
        assert_eq!(slugs.len(), unique_len, "duplicate slug detected");
    }

    #[test]
    fn role_distribution() {
        // Director: Koord + Komuta + Müdür
        let directors: Vec<_> = filter_by_role(AvkAgentRole::Director).collect();
        assert_eq!(directors.len(), 3);

        // Senior: Code-1 + Code-2 + Merge + Hata
        let seniors: Vec<_> = filter_by_role(AvkAgentRole::Senior).collect();
        assert_eq!(seniors.len(), 4);

        // Worker: Gemini-1/2 + Kimi-1/2/3 + Codex
        let workers: Vec<_> = filter_by_role(AvkAgentRole::Worker).collect();
        assert_eq!(workers.len(), 6);

        // Toplam = 13
        assert_eq!(directors.len() + seniors.len() + workers.len(), 13);
    }

    #[test]
    fn find_by_slug_works() {
        assert_eq!(find_by_slug("koord").unwrap().role, AvkAgentRole::Director);
        assert_eq!(find_by_slug("code-2").unwrap().role, AvkAgentRole::Senior);
        assert_eq!(find_by_slug("gemini-1").unwrap().role, AvkAgentRole::Worker);
        assert!(find_by_slug("bilinmeyen").is_none());
    }

    #[test]
    fn tmux_target_format_valid() {
        // Format: avk-ofis:<window>.<pane>
        for agent in AVK_AGENTS {
            assert!(
                agent.tmux_target.starts_with("avk-ofis:"),
                "{}: tmux_target invalid prefix",
                agent.slug
            );
            assert!(
                agent.tmux_target.contains('.'),
                "{}: tmux_target missing pane index",
                agent.slug
            );
        }
    }

    #[test]
    fn role_as_str_canonical() {
        assert_eq!(AvkAgentRole::Director.as_str(), "director");
        assert_eq!(AvkAgentRole::Senior.as_str(), "senior");
        assert_eq!(AvkAgentRole::Worker.as_str(), "worker");
    }

    #[test]
    fn resolve_tier_slugs_matches_filter() {
        // FUR-4121: tier resolver CLI + server ortak kullanır, distribution
        // testiyle aynı sayılar dönmeli.
        assert_eq!(resolve_tier_slugs("director").unwrap().len(), 3);
        assert_eq!(resolve_tier_slugs("senior").unwrap().len(), 4);
        assert_eq!(resolve_tier_slugs("worker").unwrap().len(), 6);
        assert_eq!(resolve_tier_slugs("all").unwrap().len(), 13);
        assert!(resolve_tier_slugs("koord").is_none());
        assert!(resolve_tier_slugs("bilinmeyen").is_none());
    }
}
