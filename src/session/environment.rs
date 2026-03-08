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
        match std::env::var(var_name) {
            Ok(v) => Some(v),
            Err(_) => {
                tracing::warn!(
                    "Environment variable ${} is not set on host, skipping",
                    var_name
                );
                None
            }
        }
    } else {
        Some(val.to_string())
    }
}

/// Validate an env entry string and return a warning message if it references
/// a host variable that doesn't exist.
///
/// Entry formats:
/// - `KEY` (bare): pass through from host
/// - `KEY=$VAR`: resolve `$VAR` from host
/// - `KEY=literal` (no `$`): always valid
/// - `KEY=$$...`: escaped literal `$`, always valid
pub fn validate_env_entry(entry: &str) -> Option<String> {
    if let Some((_, value)) = entry.split_once('=') {
        if value.starts_with("$$") {
            // Escaped literal $, always valid
            None
        } else if let Some(var_name) = value.strip_prefix('$') {
            if var_name.is_empty() {
                Some("Warning: bare '$' in value has no variable name".to_string())
            } else if resolve_env_value(value).is_none() {
                Some(format!(
                    "Warning: ${} is not set on the host -- it will be empty in the container",
                    var_name
                ))
            } else {
                None
            }
        } else {
            // Literal value, always valid
            None
        }
    } else {
        // Bare key -- pass through from host
        if std::env::var(entry).is_err() {
            Some(format!(
                "Warning: {} is not set on the host -- it will be empty in the container",
                entry
            ))
        } else {
            None
        }
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

    // When per-session extra_env is present, it is the authoritative env list
    // (the TUI seeds it from config.sandbox.environment and the user may have
    // added, edited, or removed entries). Fall back to config only when no
    // per-session overrides exist.
    let entries: &[String] = sandbox_info
        .extra_env
        .as_deref()
        .unwrap_or(&sandbox_config.environment);

    // Always ensure the terminal defaults are present (pass-through from host)
    for &key in DEFAULT_TERMINAL_ENV_VARS {
        if seen_keys.insert(key.to_string()) {
            if let Ok(val) = std::env::var(key) {
                result.push((key.to_string(), val));
            }
        }
    }

    for entry in entries {
        if let Some((key, value)) = entry.split_once('=') {
            if seen_keys.insert(key.to_string()) {
                if let Some(resolved) = resolve_env_value(value) {
                    result.push((key.to_string(), resolved));
                }
            }
        } else {
            // Bare key -- pass through from host
            if seen_keys.insert(entry.clone()) {
                match std::env::var(entry) {
                    Ok(val) => result.push((entry.clone(), val)),
                    Err(_) => {
                        tracing::warn!(
                            "Environment variable {} is not set on host, skipping",
                            entry
                        );
                    }
                }
            }
        }
    }

    result
}

/// Resolve the effective sandbox config by merging global + active profile.
fn resolved_sandbox_config() -> super::config::SandboxConfig {
    let profile = super::config::resolve_default_profile();
    super::profile_config::resolve_config(&profile)
        .map(|c| c.sandbox)
        .unwrap_or_default()
}

/// Build docker exec environment flags from config and optional per-session extra entries.
/// Used for `docker exec` commands (shell string interpolation, hence shell-escaping).
/// Container creation uses `ContainerConfig.environment` (separate args, no escaping needed).
pub(crate) fn build_docker_env_args(sandbox: &SandboxInfo) -> String {
    let sandbox_config = resolved_sandbox_config();

    tracing::debug!(
        "build_docker_env_args: config.sandbox.environment={:?}, extra_env={:?}",
        sandbox_config.environment,
        sandbox.extra_env
    );

    let env_pairs = collect_environment(&sandbox_config, sandbox);

    tracing::debug!(
        "build_docker_env_args: resolved {} env pairs",
        env_pairs.len()
    );
    for (k, _) in &env_pairs {
        tracing::debug!("  env: {}=<set>", k);
    }

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
    fn test_collect_environment_extra_env_is_authoritative() {
        let config = SandboxConfig {
            environment: vec!["DUP_KEY=from_config".to_string()],
            ..Default::default()
        };
        let info = SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            extra_env: Some(vec!["DUP_KEY=from_session".to_string()]),
            custom_instruction: None,
        };

        let result = collect_environment(&config, &info);
        let dup_entries: Vec<_> = result.iter().filter(|(k, _)| k == "DUP_KEY").collect();
        assert_eq!(dup_entries.len(), 1);
        assert_eq!(dup_entries[0].1, "from_session");
    }

    #[test]
    fn test_collect_environment_falls_back_to_config_when_no_extra() {
        let config = SandboxConfig {
            environment: vec!["CONFIG_KEY=config_val".to_string()],
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
            .any(|(k, v)| k == "CONFIG_KEY" && v == "config_val"));
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

    #[test]
    fn test_validate_env_entry_bare_key_present() {
        std::env::set_var("AOE_TEST_VALIDATE_BARE", "exists");
        assert_eq!(validate_env_entry("AOE_TEST_VALIDATE_BARE"), None);
        std::env::remove_var("AOE_TEST_VALIDATE_BARE");
    }

    #[test]
    fn test_validate_env_entry_bare_key_missing() {
        std::env::remove_var("AOE_TEST_VALIDATE_MISSING_BARE");
        let result = validate_env_entry("AOE_TEST_VALIDATE_MISSING_BARE");
        assert!(result.is_some());
        assert!(result.unwrap().contains("AOE_TEST_VALIDATE_MISSING_BARE"));
    }

    #[test]
    fn test_validate_env_entry_key_dollar_var_present() {
        std::env::set_var("AOE_TEST_VALIDATE_REF", "value");
        assert_eq!(validate_env_entry("MY_KEY=$AOE_TEST_VALIDATE_REF"), None);
        std::env::remove_var("AOE_TEST_VALIDATE_REF");
    }

    #[test]
    fn test_validate_env_entry_key_dollar_var_missing() {
        std::env::remove_var("AOE_TEST_VALIDATE_MISSING_REF");
        let result = validate_env_entry("MY_KEY=$AOE_TEST_VALIDATE_MISSING_REF");
        assert!(result.is_some());
        assert!(result.unwrap().contains("AOE_TEST_VALIDATE_MISSING_REF"));
    }

    #[test]
    fn test_validate_env_entry_literal_value() {
        assert_eq!(validate_env_entry("MY_KEY=some_literal"), None);
    }

    #[test]
    fn test_validate_env_entry_escaped_dollar() {
        assert_eq!(validate_env_entry("MY_KEY=$$ESCAPED"), None);
    }

    #[test]
    fn test_build_docker_env_args_with_extra_env() {
        std::env::set_var("AOE_TEST_TOKEN", "secret123");
        let sandbox = SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            extra_env: Some(vec!["MY_TOKEN=$AOE_TEST_TOKEN".to_string()]),
            custom_instruction: None,
        };
        let result = build_docker_env_args(&sandbox);
        assert!(
            result.contains("MY_TOKEN"),
            "Expected MY_TOKEN in args: {}",
            result
        );
        assert!(
            result.contains("secret123"),
            "Expected secret123 in args: {}",
            result
        );
        std::env::remove_var("AOE_TEST_TOKEN");
    }

    #[test]
    fn test_build_docker_env_args_bare_key() {
        std::env::set_var("AOE_TEST_BARE", "barevalue");
        let sandbox = SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            extra_env: Some(vec!["AOE_TEST_BARE".to_string()]),
            custom_instruction: None,
        };
        let result = build_docker_env_args(&sandbox);
        assert!(
            result.contains("AOE_TEST_BARE"),
            "Expected AOE_TEST_BARE in args: {}",
            result
        );
        assert!(
            result.contains("barevalue"),
            "Expected barevalue in args: {}",
            result
        );
        std::env::remove_var("AOE_TEST_BARE");
    }
}
