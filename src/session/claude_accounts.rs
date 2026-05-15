//! Discovery of Claude config directories ("accounts") for the new-session picker.
//!
//! An "account" is any directory under `~/.claude-accounts/` that contains a
//! `settings.json` (file or symlink) and is not `_shared` or dotted. Each one
//! corresponds to a distinct Claude config directory the user can pick from
//! when starting a new session. The TUI renders one picker row per account
//! beneath the plain `claude` row, e.g.:
//!
//! ```text
//!   claude
//!   ──────
//!   claude => ForIT Main
//!   claude => ForIT Work
//! ```
//!
//! Selection writes the chosen account's directory path into
//! `Instance.claude_config_dir`; spawn-time injection prepends
//! `CLAUDE_CONFIG_DIR=<expanded>` to the host env prefix. The plain `claude`
//! row leaves `claude_config_dir` as `None` (inherit shell env, the
//! default behavior).

use std::path::{Path, PathBuf};

/// Acronym pretty-print table. Lower-case keyword => display form.
/// Matched per dash-separated segment of the account directory name so
/// `forit-main` => `ForIT Main`, `wma-work` => `WMA Work`.
const ACRONYMS: &[(&str, &str)] = &[("forit", "ForIT"), ("wma", "WMA"), ("cs", "CS")];

/// One discovered Claude account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeAccount {
    /// Directory basename (e.g. `"forit-main"`). Used as the stable
    /// identifier and as the input to `display_label`.
    pub name: String,
    /// Absolute path to the account directory, injected as
    /// `$CLAUDE_CONFIG_DIR`. Persisted to `Instance.claude_config_dir`.
    pub config_dir: PathBuf,
}

impl ClaudeAccount {
    pub fn display_label(&self) -> String {
        display_label(&self.name)
    }
}

/// Default scan root: `$HOME/.claude-accounts/`. Returns `None` if `HOME`
/// is unset (e.g. in tests on a stripped env).
pub fn default_root() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude-accounts"))
}

/// Scan `root` for valid Claude account directories. A directory is valid
/// when it:
/// - is not `_shared`,
/// - is not dotted (`.foo`),
/// - contains a `settings.json` file or symlink.
///
/// Missing root or unreadable root => empty vector (no panic). Entries are
/// sorted alphabetically by name for stable picker ordering.
pub fn discover_accounts(root: &Path) -> Vec<ClaudeAccount> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<ClaudeAccount> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            if !path.is_dir() {
                return None;
            }
            let name = path.file_name()?.to_str()?.to_string();
            if is_reserved_name(&name) {
                return None;
            }
            if !has_settings_json(&path) {
                return None;
            }
            Some(ClaudeAccount {
                name,
                config_dir: path,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn is_reserved_name(name: &str) -> bool {
    name == "_shared" || name.starts_with('.')
}

fn has_settings_json(dir: &Path) -> bool {
    let p = dir.join("settings.json");
    p.is_file() || p.is_symlink()
}

/// Pretty display label derived from a dash-separated directory name.
/// Each segment is matched against `ACRONYMS` (case-insensitive); a hit
/// substitutes the canonical mixed-case form, a miss is title-cased.
/// Segments are joined with a single space.
///
/// Examples:
/// - `forit-main`   => `ForIT Main`
/// - `wma-work`     => `WMA Work`
/// - `pivot-main`   => `Pivot Main`
/// - `forit-backup` => `ForIT Backup`
pub fn display_label(name: &str) -> String {
    name.split('-')
        .map(format_segment)
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_segment(segment: &str) -> String {
    let lower = segment.to_ascii_lowercase();
    for (key, repl) in ACRONYMS {
        if lower == *key {
            return (*repl).to_string();
        }
    }
    title_case(segment)
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => {
            first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_account(root: &Path, name: &str) {
        let d = root.join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("settings.json"), "{}").unwrap();
    }

    fn make_dir_only(root: &Path, name: &str) {
        fs::create_dir_all(root.join(name)).unwrap();
    }

    #[test]
    fn discover_skips_reserved_and_invalid() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        make_account(root, "forit-main");
        make_account(root, "wma-work");
        make_account(root, "_shared"); // reserved
        make_account(root, ".cache"); // dotted
        make_dir_only(root, "no-settings"); // missing settings.json

        let found = discover_accounts(root);
        let names: Vec<&str> = found.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["forit-main", "wma-work"]);
    }

    #[test]
    fn discover_returns_empty_on_missing_root() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        assert!(discover_accounts(&missing).is_empty());
    }

    #[test]
    fn discover_results_are_sorted() {
        let tmp = TempDir::new().unwrap();
        make_account(tmp.path(), "wma-work");
        make_account(tmp.path(), "forit-main");
        make_account(tmp.path(), "pivot-main");

        let names: Vec<String> = discover_accounts(tmp.path())
            .into_iter()
            .map(|a| a.name)
            .collect();
        assert_eq!(names, vec!["forit-main", "pivot-main", "wma-work"]);
    }

    #[test]
    fn account_carries_full_path() {
        let tmp = TempDir::new().unwrap();
        make_account(tmp.path(), "forit-main");
        let found = discover_accounts(tmp.path());
        assert_eq!(found[0].config_dir, tmp.path().join("forit-main"));
    }

    #[test]
    fn display_label_uses_acronym_table() {
        assert_eq!(display_label("forit-main"), "ForIT Main");
        assert_eq!(display_label("forit-work"), "ForIT Work");
        assert_eq!(display_label("forit-backup"), "ForIT Backup");
        assert_eq!(display_label("wma-work"), "WMA Work");
        assert_eq!(display_label("pivot-main"), "Pivot Main");
    }

    #[test]
    fn display_label_handles_unknown_words() {
        assert_eq!(display_label("alice-personal"), "Alice Personal");
        assert_eq!(display_label("single"), "Single");
    }

    #[test]
    fn display_label_is_case_insensitive_for_acronyms() {
        assert_eq!(display_label("FORIT-Main"), "ForIT Main");
        assert_eq!(display_label("Wma-WORK"), "WMA Work");
    }

    #[test]
    fn settings_json_symlink_counts() {
        let tmp = TempDir::new().unwrap();
        let shared = tmp.path().join("_shared");
        fs::create_dir_all(&shared).unwrap();
        let shared_settings = shared.join("settings.json");
        fs::write(&shared_settings, "{}").unwrap();

        let acct = tmp.path().join("forit-main");
        fs::create_dir_all(&acct).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&shared_settings, acct.join("settings.json")).unwrap();
        #[cfg(not(unix))]
        fs::write(acct.join("settings.json"), "{}").unwrap();

        let found = discover_accounts(tmp.path());
        let names: Vec<&str> = found.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["forit-main"]);
    }
}
