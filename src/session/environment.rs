//! Environment variable helpers for session instances.
//!
//! Pure functions for building environment variable arguments used when
//! launching tools inside Docker containers.

use super::config::SandboxConfig;
use super::instance::SandboxInfo;
use crate::containers::container_interface::EnvEntry;

/// Terminal environment variables that are always passed through for proper UI/theming
pub(crate) const DEFAULT_TERMINAL_ENV_VARS: &[&str] =
    &["TERM", "COLORTERM", "FORCE_COLOR", "NO_COLOR"];

/// Returns the user's preferred shell from `$SHELL`, falling back to `bash`.
///
/// Used for host-side command wrappers (agent launch, local hook execution)
/// so that the user's PATH and rc-file sourcing work correctly. Container
/// contexts should keep using a fixed shell since the user shell may not be
/// installed inside the image.
pub(crate) fn user_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "bash".to_string())
}

/// Shells whose quoting rules are incompatible with POSIX `'\''` escaping.
const NON_POSIX_SHELLS: &[&str] = &["fish", "nu", "nushell", "pwsh", "powershell"];

/// Like [`user_shell`], but falls back to `bash` when the user's shell is
/// non-POSIX (e.g. fish, nushell, pwsh). Use this for command wrappers that
/// rely on POSIX single-quote escaping (`'\''`).
pub(crate) fn user_posix_shell() -> String {
    let shell = user_shell();
    let basename = std::path::Path::new(&shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&shell);
    if NON_POSIX_SHELLS.contains(&basename) {
        "bash".to_string()
    } else {
        shell
    }
}

/// Shell-escape a value for safe interpolation into a shell command string.
///
/// Uses single-quote escaping: inside single quotes ALL characters are literal
/// except `'` itself, which is escaped via the POSIX `'\''` technique. This is
/// the most robust approach -- it prevents expansion of `$`, `` ` ``, `\`, `!`,
/// and every other shell metacharacter in one shot.
///
/// Newlines and carriage returns are replaced with literal `\n` / `\r` text to
/// keep the command on a single line (required for tmux session commands).
pub(crate) fn shell_escape(val: &str) -> String {
    let val = val.replace('\n', "\\n").replace('\r', "\\r");
    let escaped = val.replace('\'', "'\\''");
    format!("'{}'", escaped)
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
/// - `KEY` (no `=`) -- pass through from host (inherited, not in argv)
/// - `KEY=$VAR` -- read from host env (inherited, not in argv)
/// - `KEY=literal` -- literal value (appears in argv, safe for non-secrets)
///
/// Returns `EnvEntry` values that distinguish inherited-from-host entries
/// (which use Docker `-e KEY` to avoid leaking secrets in argv/ps) from
/// literal entries (which use `-e KEY=VALUE`).
///
/// Deduplicates by key (first wins).
pub(crate) fn collect_environment(
    sandbox_config: &SandboxConfig,
    sandbox_info: &SandboxInfo,
) -> Vec<EnvEntry> {
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
                result.push(EnvEntry::Inherit {
                    key: key.to_string(),
                    value: val,
                });
            }
        }
    }

    for entry in entries {
        if let Some((key, value)) = entry.split_once('=') {
            if seen_keys.insert(key.to_string()) {
                if let Some(rest) = value.strip_prefix("$$") {
                    // Escaped literal $, e.g. KEY=$$FOO -> KEY=$FOO
                    let literal = format!("${}", rest);
                    result.push(EnvEntry::Literal {
                        key: key.to_string(),
                        value: literal,
                    });
                } else if value.starts_with('$') {
                    // Host env reference, e.g. GH_TOKEN=$GH_TOKEN
                    if let Some(resolved) = resolve_env_value(value) {
                        result.push(EnvEntry::Inherit {
                            key: key.to_string(),
                            value: resolved,
                        });
                    }
                } else {
                    // Literal value, e.g. TERM=xterm-256color
                    result.push(EnvEntry::Literal {
                        key: key.to_string(),
                        value: value.to_string(),
                    });
                }
            }
        } else {
            // Bare key -- pass through from host
            if seen_keys.insert(entry.clone()) {
                match std::env::var(entry) {
                    Ok(val) => {
                        result.push(EnvEntry::Inherit {
                            key: entry.clone(),
                            value: val,
                        });
                    }
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

/// Resolve the effective sandbox config by merging global + active profile + repo.
fn resolved_sandbox_config(project_path: &std::path::Path) -> super::config::SandboxConfig {
    let profile = super::config::resolve_default_profile();
    super::repo_config::resolve_config_with_repo(&profile, project_path)
        .map(|c| c.sandbox)
        .unwrap_or_default()
}

/// Build docker exec environment flags from config and optional per-session extra entries.
/// Used for `docker exec` commands (shell string interpolation, hence shell-escaping).
/// Container creation uses `ContainerConfig.environment` (separate args, no escaping needed).
///
/// Docker exec commands run inside tmux, so there is no reliable way to inject
/// env vars into the Docker CLI process without putting values in the command
/// string. This function therefore always uses `-e KEY=VALUE` (the pre-existing
/// behavior), keeping secrets in the shell command but ensuring they always
/// reach the container.
///
/// The `docker run` path (container creation) is protected separately via
/// `Command::env()` in `run_create`, which keeps secrets out of argv entirely.
pub(crate) fn build_docker_env_args(
    sandbox: &SandboxInfo,
    project_path: &std::path::Path,
) -> String {
    let sandbox_config = resolved_sandbox_config(project_path);

    tracing::debug!(
        "build_docker_env_args: config.sandbox.environment={:?}, extra_env={:?}",
        sandbox_config.environment,
        sandbox.extra_env
    );

    let env_entries = collect_environment(&sandbox_config, sandbox);

    tracing::debug!(
        "build_docker_env_args: resolved {} env entries",
        env_entries.len()
    );
    for entry in &env_entries {
        tracing::debug!("  env: {}=<set>", entry.key());
    }

    // Always pass values explicitly for docker exec via tmux.
    // We cannot use `-e KEY` (inherit) here because docker exec runs
    // inside a tmux session whose shell environment may not have the
    // variable (tmux server may have started before the var was set).
    let args: Vec<String> = env_entries
        .iter()
        .map(|entry| format!("-e {}={}", entry.key(), shell_escape(entry.value())))
        .collect();

    args.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_escape_simple() {
        assert_eq!(shell_escape("hello"), "'hello'");
    }

    #[test]
    fn test_shell_escape_apostrophe() {
        assert_eq!(shell_escape("Don't do that"), "'Don'\\''t do that'");
    }

    #[test]
    fn test_shell_escape_double_quotes() {
        // Double quotes are literal inside single quotes -- no escaping needed
        assert_eq!(shell_escape("say \"hello\""), "'say \"hello\"'");
    }

    #[test]
    fn test_shell_escape_backslash() {
        // Backslashes are literal inside single quotes -- no escaping needed
        assert_eq!(shell_escape("path\\to\\file"), "'path\\to\\file'");
    }

    #[test]
    fn test_shell_escape_dollar() {
        // $ is literal inside single quotes -- no expansion
        assert_eq!(shell_escape("$HOME/path"), "'$HOME/path'");
    }

    #[test]
    fn test_shell_escape_backtick() {
        // Backticks are literal inside single quotes -- no command substitution
        assert_eq!(shell_escape("run `cmd`"), "'run `cmd`'");
    }

    #[test]
    fn test_shell_escape_exclamation() {
        // ! is literal inside single quotes -- no history expansion
        assert_eq!(shell_escape("hello!"), "'hello!'");
    }

    #[test]
    fn test_shell_escape_newline() {
        assert_eq!(shell_escape("line1\nline2"), "'line1\\nline2'");
    }

    #[test]
    fn test_shell_escape_carriage_return() {
        assert_eq!(shell_escape("line1\rline2"), "'line1\\rline2'");
    }

    #[test]
    fn test_shell_escape_multiline_instruction() {
        let instruction = "First instruction.\nSecond instruction.\nThird instruction.";
        let escaped = shell_escape(instruction);
        assert_eq!(
            escaped,
            "'First instruction.\\nSecond instruction.\\nThird instruction.'"
        );
        assert!(!escaped.contains('\n'));
    }

    #[test]
    fn test_shell_escape_crlf() {
        assert_eq!(shell_escape("line1\r\nline2"), "'line1\\r\\nline2'");
    }

    #[test]
    fn test_shell_escape_combined() {
        let input = "Say \"hello\"\nRun `echo $HOME`";
        let escaped = shell_escape(input);
        assert_eq!(escaped, "'Say \"hello\"\\nRun `echo $HOME`'");
        assert!(!escaped.contains('\n'));
    }

    #[test]
    fn test_shell_escape_mixed_quotes() {
        // Both apostrophes and double quotes
        let input = "He said \"don't\"";
        let escaped = shell_escape(input);
        assert_eq!(escaped, "'He said \"don'\\''t\"'");
    }

    /// Helper to find an entry by key and check its value
    fn find_entry<'a>(entries: &'a [EnvEntry], key: &str) -> Option<&'a EnvEntry> {
        entries.iter().find(|e| e.key() == key)
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
            cpu_limit: None,
            memory_limit: None,
            port_mappings: None,
            mount_ssh: None,
            volume_ignores: None,
            extra_volumes: None,
        };

        let result = collect_environment(&config, &info);
        let entry = find_entry(&result, "AOE_TEST_ENV_PT").expect("AOE_TEST_ENV_PT not found");
        assert_eq!(entry.value(), "test_value");
        assert!(matches!(entry, EnvEntry::Inherit { .. }));
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
            cpu_limit: None,
            memory_limit: None,
            port_mappings: None,
            mount_ssh: None,
            volume_ignores: None,
            extra_volumes: None,
        };

        let result = collect_environment(&config, &info);
        let entry = find_entry(&result, "MY_KEY").expect("MY_KEY not found");
        assert_eq!(entry.value(), "my_value");
        assert!(matches!(entry, EnvEntry::Literal { .. }));
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
            cpu_limit: None,
            memory_limit: None,
            port_mappings: None,
            mount_ssh: None,
            volume_ignores: None,
            extra_volumes: None,
        };

        let result = collect_environment(&config, &info);
        let extra = find_entry(&result, "AOE_TEST_EXTRA").expect("AOE_TEST_EXTRA not found");
        assert_eq!(extra.value(), "extra_val");
        assert!(matches!(extra, EnvEntry::Inherit { .. }));
        let foo = find_entry(&result, "FOO").expect("FOO not found");
        assert_eq!(foo.value(), "bar");
        assert!(matches!(foo, EnvEntry::Literal { .. }));
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
            cpu_limit: None,
            memory_limit: None,
            port_mappings: None,
            mount_ssh: None,
            volume_ignores: None,
            extra_volumes: None,
        };

        let result = collect_environment(&config, &info);
        let dup_entries: Vec<_> = result.iter().filter(|e| e.key() == "DUP_KEY").collect();
        assert_eq!(dup_entries.len(), 1);
        assert_eq!(dup_entries[0].value(), "from_session");
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
            cpu_limit: None,
            memory_limit: None,
            port_mappings: None,
            mount_ssh: None,
            volume_ignores: None,
            extra_volumes: None,
        };

        let result = collect_environment(&config, &info);
        let entry = find_entry(&result, "CONFIG_KEY").expect("CONFIG_KEY not found");
        assert_eq!(entry.value(), "config_val");
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
            cpu_limit: None,
            memory_limit: None,
            port_mappings: None,
            mount_ssh: None,
            volume_ignores: None,
            extra_volumes: None,
        };

        let result = collect_environment(&config, &info);
        let entry = find_entry(&result, "INJECTED").expect("INJECTED not found");
        assert_eq!(entry.value(), "host_val");
        assert!(matches!(entry, EnvEntry::Inherit { .. }));
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
            cpu_limit: None,
            memory_limit: None,
            port_mappings: None,
            mount_ssh: None,
            volume_ignores: None,
            extra_volumes: None,
        };

        let result = collect_environment(&config, &info);
        let entry = find_entry(&result, "ESCAPED").expect("ESCAPED not found");
        assert_eq!(entry.value(), "$LITERAL");
        assert!(matches!(entry, EnvEntry::Literal { .. }));
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
    fn test_build_docker_env_args_passes_values_explicitly() {
        // Docker exec runs inside tmux, so values must always be passed
        // explicitly (tmux's env may not have the variable).
        std::env::set_var("AOE_TEST_TOKEN", "secret123");
        let sandbox = SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            extra_env: Some(vec!["AOE_TEST_TOKEN=$AOE_TEST_TOKEN".to_string()]),
            custom_instruction: None,
            cpu_limit: None,
            memory_limit: None,
            port_mappings: None,
            mount_ssh: None,
            volume_ignores: None,
            extra_volumes: None,
        };
        let result = build_docker_env_args(&sandbox, std::path::Path::new("/nonexistent"));
        assert!(
            result.contains("AOE_TEST_TOKEN"),
            "Expected AOE_TEST_TOKEN in args: {}",
            result
        );
        // Value must be present (docker exec via tmux needs explicit values)
        assert!(
            result.contains("secret123"),
            "Expected value in args for docker exec: {}",
            result
        );
        std::env::remove_var("AOE_TEST_TOKEN");
    }

    #[test]
    fn test_build_docker_env_args_different_key() {
        std::env::set_var("AOE_TEST_SOURCE", "secret456");
        let sandbox = SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            extra_env: Some(vec!["MY_MAPPED=$AOE_TEST_SOURCE".to_string()]),
            custom_instruction: None,
            cpu_limit: None,
            memory_limit: None,
            port_mappings: None,
            mount_ssh: None,
            volume_ignores: None,
            extra_volumes: None,
        };
        let result = build_docker_env_args(&sandbox, std::path::Path::new("/nonexistent"));
        assert!(
            result.contains("MY_MAPPED"),
            "Expected MY_MAPPED in args: {}",
            result
        );
        assert!(
            result.contains("secret456"),
            "Expected value in args: {}",
            result
        );
        std::env::remove_var("AOE_TEST_SOURCE");
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
            cpu_limit: None,
            memory_limit: None,
            port_mappings: None,
            mount_ssh: None,
            volume_ignores: None,
            extra_volumes: None,
        };
        let result = build_docker_env_args(&sandbox, std::path::Path::new("/nonexistent"));
        assert!(
            result.contains("AOE_TEST_BARE"),
            "Expected AOE_TEST_BARE in args: {}",
            result
        );
        assert!(
            result.contains("barevalue"),
            "Expected value in args for docker exec: {}",
            result
        );
        std::env::remove_var("AOE_TEST_BARE");
    }

    #[test]
    #[serial_test::serial(shell_env)]
    fn test_user_shell_reads_env() {
        let original = std::env::var("SHELL").ok();
        std::env::set_var("SHELL", "/bin/zsh");
        assert_eq!(user_shell(), "/bin/zsh");
        match original {
            Some(v) => std::env::set_var("SHELL", v),
            None => std::env::remove_var("SHELL"),
        }
    }

    #[test]
    #[serial_test::serial(shell_env)]
    fn test_user_shell_fallback() {
        let original = std::env::var("SHELL").ok();
        std::env::remove_var("SHELL");
        assert_eq!(user_shell(), "bash");
        if let Some(v) = original {
            std::env::set_var("SHELL", v);
        }
    }

    #[test]
    #[serial_test::serial(shell_env)]
    fn test_user_shell_empty_falls_back() {
        let original = std::env::var("SHELL").ok();
        std::env::set_var("SHELL", "  ");
        assert_eq!(user_shell(), "bash");
        match original {
            Some(v) => std::env::set_var("SHELL", v),
            None => std::env::remove_var("SHELL"),
        }
    }

    #[test]
    #[serial_test::serial(shell_env)]
    fn test_user_posix_shell_returns_posix() {
        let original = std::env::var("SHELL").ok();
        std::env::set_var("SHELL", "/bin/zsh");
        assert_eq!(user_posix_shell(), "/bin/zsh");
        match original {
            Some(v) => std::env::set_var("SHELL", v),
            None => std::env::remove_var("SHELL"),
        }
    }

    #[test]
    #[serial_test::serial(shell_env)]
    fn test_user_posix_shell_falls_back_for_fish() {
        let original = std::env::var("SHELL").ok();
        std::env::set_var("SHELL", "/usr/bin/fish");
        assert_eq!(user_posix_shell(), "bash");
        match original {
            Some(v) => std::env::set_var("SHELL", v),
            None => std::env::remove_var("SHELL"),
        }
    }

    #[test]
    #[serial_test::serial(shell_env)]
    fn test_user_posix_shell_falls_back_for_nu() {
        let original = std::env::var("SHELL").ok();
        std::env::set_var("SHELL", "/usr/bin/nu");
        assert_eq!(user_posix_shell(), "bash");
        match original {
            Some(v) => std::env::set_var("SHELL", v),
            None => std::env::remove_var("SHELL"),
        }
    }
}
