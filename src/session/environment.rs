//! Environment variable helpers for session instances.
//!
//! Pure functions for building environment variable arguments used when
//! launching tools inside Docker containers.

use super::config::SandboxConfig;
use super::instance::SandboxInfo;

/// Terminal environment variables that are always passed through for proper UI/theming
pub(crate) const DEFAULT_TERMINAL_ENV_VARS: &[&str] =
    &["TERM", "COLORTERM", "FORCE_COLOR", "NO_COLOR"];

/// Shell-escape a value for safe interpolation into a shell command string.
/// Uses double-quote escaping so values can be nested inside `bash -c '...'`
/// (single quotes in the outer wrapper are literal, double quotes work inside).
pub(crate) fn shell_escape(val: &str) -> String {
    let escaped = val
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{}\"", escaped)
}

/// Resolve an environment value. If the value starts with `$`, read the
/// named variable from the host environment (use `$$` to escape a literal `$`).
/// Otherwise return the literal value.
pub(crate) fn resolve_env_value(val: &str) -> Option<String> {
    if let Some(rest) = val.strip_prefix("$$") {
        Some(format!("${}", rest))
    } else if let Some(var_name) = val.strip_prefix('$') {
        std::env::var(var_name).ok()
    } else {
        Some(val.to_string())
    }
}

/// Collect all environment entries from defaults, global config, and per-session extras.
///
/// Each entry is either:
/// - `KEY` (no `=`) -- pass through from host
/// - `KEY=VALUE` -- set explicit value (VALUE supports `$HOST_VAR` and `$$` escaping)
///
/// Returns resolved `(key, value)` pairs. Deduplicates by key (first wins).
pub(crate) fn collect_environment(
    sandbox_config: &SandboxConfig,
    sandbox_info: &SandboxInfo,
) -> Vec<(String, String)> {
    let mut seen_keys = std::collections::HashSet::new();
    let mut result = Vec::new();

    let sources: &[&[String]] = &[&sandbox_config.environment];
    let extra = sandbox_info.extra_env.as_deref().unwrap_or(&[]);

    // Process DEFAULT_TERMINAL_ENV_VARS first (pass-through)
    for &key in DEFAULT_TERMINAL_ENV_VARS {
        if seen_keys.insert(key.to_string()) {
            if let Ok(val) = std::env::var(key) {
                result.push((key.to_string(), val));
            }
        }
    }

    // Process config entries, then per-session extras
    for entries in sources.iter().chain(std::iter::once(&extra)) {
        for entry in *entries {
            if let Some((key, value)) = entry.split_once('=') {
                if seen_keys.insert(key.to_string()) {
                    if let Some(resolved) = resolve_env_value(value) {
                        result.push((key.to_string(), resolved));
                    }
                }
            } else {
                // Bare key -- pass through from host
                if seen_keys.insert(entry.clone()) {
                    if let Ok(val) = std::env::var(entry) {
                        result.push((entry.clone(), val));
                    }
                }
            }
        }
    }

    result
}

/// Build docker exec environment flags from config and optional per-session extra entries.
/// Used for `docker exec` commands (shell string interpolation, hence shell-escaping).
/// Container creation uses `ContainerConfig.environment` (separate args, no escaping needed).
pub(crate) fn build_docker_env_args(sandbox: &SandboxInfo) -> String {
    let config = super::config::Config::load().unwrap_or_default();

    let env_pairs = collect_environment(&config.sandbox, sandbox);

    let args: Vec<String> = env_pairs
        .iter()
        .map(|(key, val)| format!("-e {}={}", key, shell_escape(val)))
        .collect();

    args.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_escape_simple() {
        assert_eq!(shell_escape("hello"), "\"hello\"");
    }

    #[test]
    fn test_shell_escape_quotes() {
        assert_eq!(shell_escape("say \"hello\""), "\"say \\\"hello\\\"\"");
    }

    #[test]
    fn test_shell_escape_backslash() {
        assert_eq!(shell_escape("path\\to\\file"), "\"path\\\\to\\\\file\"");
    }

    #[test]
    fn test_shell_escape_dollar() {
        assert_eq!(shell_escape("$HOME/path"), "\"\\$HOME/path\"");
    }

    #[test]
    fn test_shell_escape_backtick() {
        assert_eq!(shell_escape("run `cmd`"), "\"run \\`cmd\\`\"");
    }

    #[test]
    fn test_shell_escape_newline() {
        assert_eq!(shell_escape("line1\nline2"), "\"line1\\nline2\"");
    }

    #[test]
    fn test_shell_escape_carriage_return() {
        assert_eq!(shell_escape("line1\rline2"), "\"line1\\rline2\"");
    }

    #[test]
    fn test_shell_escape_multiline_instruction() {
        let instruction = "First instruction.\nSecond instruction.\nThird instruction.";
        let escaped = shell_escape(instruction);
        assert_eq!(
            escaped,
            "\"First instruction.\\nSecond instruction.\\nThird instruction.\""
        );
        assert!(!escaped.contains('\n'));
    }

    #[test]
    fn test_shell_escape_crlf() {
        assert_eq!(shell_escape("line1\r\nline2"), "\"line1\\r\\nline2\"");
    }

    #[test]
    fn test_shell_escape_combined() {
        let input = "Say \"hello\"\nRun `echo $HOME`";
        let escaped = shell_escape(input);
        assert_eq!(escaped, "\"Say \\\"hello\\\"\\nRun \\`echo \\$HOME\\`\"");
        assert!(!escaped.contains('\n'));
    }

    #[test]
    fn test_collect_environment_passthrough() {
        std::env::set_var("AOE_TEST_ENV_PT", "test_value");
        let config = SandboxConfig {
            environment: vec!["AOE_TEST_ENV_PT".to_string()],
            ..Default::default()
        };
        let info = SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            extra_env: None,
            custom_instruction: None,
        };

        let result = collect_environment(&config, &info);
        assert!(result
            .iter()
            .any(|(k, v)| k == "AOE_TEST_ENV_PT" && v == "test_value"));
        std::env::remove_var("AOE_TEST_ENV_PT");
    }

    #[test]
    fn test_collect_environment_key_value() {
        let config = SandboxConfig {
            environment: vec!["MY_KEY=my_value".to_string()],
            ..Default::default()
        };
        let info = SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            extra_env: None,
            custom_instruction: None,
        };

        let result = collect_environment(&config, &info);
        assert!(result.iter().any(|(k, v)| k == "MY_KEY" && v == "my_value"));
    }

    #[test]
    fn test_collect_environment_extra_env() {
        std::env::set_var("AOE_TEST_EXTRA", "extra_val");
        let config = SandboxConfig::default();
        let info = SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            extra_env: Some(vec!["AOE_TEST_EXTRA".to_string(), "FOO=bar".to_string()]),
            custom_instruction: None,
        };

        let result = collect_environment(&config, &info);
        assert!(result
            .iter()
            .any(|(k, v)| k == "AOE_TEST_EXTRA" && v == "extra_val"));
        assert!(result.iter().any(|(k, v)| k == "FOO" && v == "bar"));
        std::env::remove_var("AOE_TEST_EXTRA");
    }

    #[test]
    fn test_collect_environment_dedup_first_wins() {
        let config = SandboxConfig {
            environment: vec!["DUP_KEY=first".to_string()],
            ..Default::default()
        };
        let info = SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            extra_env: Some(vec!["DUP_KEY=second".to_string()]),
            custom_instruction: None,
        };

        let result = collect_environment(&config, &info);
        let dup_entries: Vec<_> = result.iter().filter(|(k, _)| k == "DUP_KEY").collect();
        assert_eq!(dup_entries.len(), 1);
        assert_eq!(dup_entries[0].1, "first");
    }

    #[test]
    fn test_collect_environment_dollar_ref() {
        std::env::set_var("AOE_TEST_HOST_REF", "host_val");
        let config = SandboxConfig {
            environment: vec!["INJECTED=$AOE_TEST_HOST_REF".to_string()],
            ..Default::default()
        };
        let info = SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            extra_env: None,
            custom_instruction: None,
        };

        let result = collect_environment(&config, &info);
        assert!(result
            .iter()
            .any(|(k, v)| k == "INJECTED" && v == "host_val"));
        std::env::remove_var("AOE_TEST_HOST_REF");
    }

    #[test]
    fn test_collect_environment_dollar_dollar_escape() {
        let config = SandboxConfig {
            environment: vec!["ESCAPED=$$LITERAL".to_string()],
            ..Default::default()
        };
        let info = SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            extra_env: None,
            custom_instruction: None,
        };

        let result = collect_environment(&config, &info);
        assert!(result
            .iter()
            .any(|(k, v)| k == "ESCAPED" && v == "$LITERAL"));
    }
}
