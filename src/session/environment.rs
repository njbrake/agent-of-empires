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

/// Resolve an environment_values entry. If the value starts with `$`, read the
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

/// Collect all environment variable keys from defaults, global config, and per-session extras.
pub(crate) fn collect_env_keys(
    sandbox_config: &SandboxConfig,
    sandbox_info: &SandboxInfo,
) -> Vec<String> {
    let mut env_keys: Vec<String> = DEFAULT_TERMINAL_ENV_VARS
        .iter()
        .map(|s| s.to_string())
        .collect();

    for key in &sandbox_config.environment {
        if !env_keys.contains(key) {
            env_keys.push(key.clone());
        }
    }

    if let Some(extra_keys) = &sandbox_info.extra_env_keys {
        for key in extra_keys {
            if !env_keys.contains(key) {
                env_keys.push(key.clone());
            }
        }
    }

    env_keys
}

/// Collect all key=value environment pairs from global config and per-session extras.
pub(crate) fn collect_env_values(
    sandbox_config: &SandboxConfig,
    sandbox_info: &SandboxInfo,
) -> Vec<(String, String)> {
    let mut values = Vec::new();

    for (key, val) in &sandbox_config.environment_values {
        if let Some(resolved) = resolve_env_value(val) {
            values.push((key.clone(), resolved));
        }
    }

    if let Some(extra_vals) = &sandbox_info.extra_env_values {
        for (key, val) in extra_vals {
            if let Some(resolved) = resolve_env_value(val) {
                values.push((key.clone(), resolved));
            }
        }
    }

    values
}

/// Build docker exec environment flags from config and optional per-session extra keys.
/// Used for `docker exec` commands (shell string interpolation, hence shell-escaping).
/// Container creation uses `ContainerConfig.environment` (separate args, no escaping needed).
pub(crate) fn build_docker_env_args(sandbox: &SandboxInfo) -> String {
    let config = super::config::Config::load().unwrap_or_default();

    let env_keys = collect_env_keys(&config.sandbox, sandbox);

    let mut args: Vec<String> = env_keys
        .iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|val| format!("-e {}={}", key, shell_escape(&val)))
        })
        .collect();

    for (key, resolved) in collect_env_values(&config.sandbox, sandbox) {
        args.push(format!("-e {}={}", key, shell_escape(&resolved)));
    }

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
        // Verify no actual newlines in the escaped string
        assert!(!escaped.contains('\n'));
    }

    #[test]
    fn test_shell_escape_crlf() {
        assert_eq!(shell_escape("line1\r\nline2"), "\"line1\\r\\nline2\"");
    }

    #[test]
    fn test_shell_escape_combined() {
        // Test a complex string with multiple special characters
        let input = "Say \"hello\"\nRun `echo $HOME`";
        let escaped = shell_escape(input);
        assert_eq!(escaped, "\"Say \\\"hello\\\"\\nRun \\`echo \\$HOME\\`\"");
        assert!(!escaped.contains('\n'));
    }
}
