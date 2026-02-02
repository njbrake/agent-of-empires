//! Integration tests for repo config loading, trust system, and hook execution.

use serial_test::serial;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Set HOME and XDG_CONFIG_HOME to a temp directory for test isolation.
///
/// # Safety caveat
/// `set_var` is not thread-safe. Tests calling this must use `#[serial]` to
/// ensure no concurrent test is reading the environment at the same time.
fn setup_temp_home(temp: &Path) {
    std::env::set_var("HOME", temp);
    #[cfg(target_os = "linux")]
    std::env::set_var("XDG_CONFIG_HOME", temp.join(".config"));
}

/// Helper to set up a temp dir with `.aoe/config.toml`.
fn setup_repo_config(content: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let aoe_dir = tmp.path().join(".aoe");
    fs::create_dir_all(&aoe_dir).unwrap();
    fs::write(aoe_dir.join("config.toml"), content).unwrap();
    tmp
}

#[test]
fn test_load_repo_config_from_temp_dir() {
    let tmp = setup_repo_config(
        r#"
[hooks]
on_create = ["echo setup"]
on_launch = ["echo start"]

[session]
default_tool = "claude"
"#,
    );

    let config = agent_of_empires::session::repo_config::load_repo_config(tmp.path())
        .unwrap()
        .unwrap();

    let hooks = config.hooks.unwrap();
    assert_eq!(hooks.on_create, vec!["echo setup"]);
    assert_eq!(hooks.on_launch, vec!["echo start"]);
    assert_eq!(
        config.session.unwrap().default_tool,
        Some("claude".to_string())
    );
}

#[test]
fn test_load_repo_config_empty_file() {
    let tmp = setup_repo_config("");
    let config = agent_of_empires::session::repo_config::load_repo_config(tmp.path()).unwrap();
    assert!(config.is_none());
}

#[test]
fn test_load_repo_config_comments_only() {
    let tmp = setup_repo_config(agent_of_empires::session::repo_config::INIT_TEMPLATE);
    let config = agent_of_empires::session::repo_config::load_repo_config(tmp.path())
        .unwrap()
        .unwrap();
    // All-commented template should parse as empty config
    assert!(config.hooks.is_none());
    assert!(config.session.is_none());
}

#[test]
#[serial]
fn test_trust_untrust_cycle() {
    let temp_home = TempDir::new().unwrap();
    setup_temp_home(temp_home.path());

    let project_dir = TempDir::new().unwrap();
    let project_path = project_dir.path();
    let hooks_hash = "test_hash_123";

    // Initially not trusted
    let is_trusted =
        agent_of_empires::session::repo_config::is_repo_trusted(project_path, hooks_hash).unwrap();
    assert!(!is_trusted);

    // Trust it
    agent_of_empires::session::repo_config::trust_repo(project_path, hooks_hash).unwrap();
    let is_trusted =
        agent_of_empires::session::repo_config::is_repo_trusted(project_path, hooks_hash).unwrap();
    assert!(is_trusted);

    // Different hash should not be trusted
    let is_trusted =
        agent_of_empires::session::repo_config::is_repo_trusted(project_path, "different_hash")
            .unwrap();
    assert!(!is_trusted);

    // Re-trust with new hash (simulating hooks changed)
    agent_of_empires::session::repo_config::trust_repo(project_path, "new_hash").unwrap();
    // Old hash no longer trusted
    let is_trusted =
        agent_of_empires::session::repo_config::is_repo_trusted(project_path, hooks_hash).unwrap();
    assert!(!is_trusted);
    // New hash is trusted
    let is_trusted =
        agent_of_empires::session::repo_config::is_repo_trusted(project_path, "new_hash").unwrap();
    assert!(is_trusted);
}

#[test]
fn test_hook_execution_simple_echo() {
    let tmp = TempDir::new().unwrap();
    let marker = tmp.path().join("hook_ran");

    let cmd = format!("touch {}", marker.display());
    agent_of_empires::session::repo_config::execute_hooks(&[cmd], tmp.path()).unwrap();

    assert!(marker.exists());
}

#[test]
fn test_hook_execution_failure() {
    let tmp = TempDir::new().unwrap();
    let result =
        agent_of_empires::session::repo_config::execute_hooks(&["exit 1".to_string()], tmp.path());
    assert!(result.is_err());
}

#[test]
fn test_changed_hooks_invalidate_trust() {
    use agent_of_empires::session::repo_config::{compute_hooks_hash, HooksConfig};

    let hooks_v1 = HooksConfig {
        on_create: vec!["npm install".to_string()],
        on_launch: vec![],
    };
    let hooks_v2 = HooksConfig {
        on_create: vec!["npm install".to_string(), "npm run build".to_string()],
        on_launch: vec![],
    };

    let hash_v1 = compute_hooks_hash(&hooks_v1);
    let hash_v2 = compute_hooks_hash(&hooks_v2);
    assert_ne!(
        hash_v1, hash_v2,
        "different hooks should produce different hashes"
    );
}

#[test]
#[serial]
fn test_hook_trust_invalidated_on_config_change() {
    use agent_of_empires::session::repo_config::{check_hook_trust, trust_repo, HookTrustStatus};

    let temp_home = TempDir::new().unwrap();
    setup_temp_home(temp_home.path());

    // Create a repo with hooks
    let repo = setup_repo_config(
        r#"
[hooks]
on_create = ["echo setup"]
"#,
    );

    // Initially untrusted
    let status = check_hook_trust(repo.path()).unwrap();
    assert!(
        matches!(status, HookTrustStatus::NeedsTrust { .. }),
        "Hooks should initially need trust"
    );

    // Trust the hooks
    if let HookTrustStatus::NeedsTrust { hooks_hash, .. } = &status {
        trust_repo(repo.path(), hooks_hash).unwrap();
    }

    // Now should be trusted
    let status = check_hook_trust(repo.path()).unwrap();
    assert!(
        matches!(status, HookTrustStatus::Trusted(_)),
        "Hooks should be trusted after trust_repo"
    );

    // Modify the hooks config
    let aoe_dir = repo.path().join(".aoe");
    fs::write(
        aoe_dir.join("config.toml"),
        r#"
[hooks]
on_create = ["echo setup", "echo extra"]
"#,
    )
    .unwrap();

    // Should no longer be trusted (hash changed)
    let status = check_hook_trust(repo.path()).unwrap();
    assert!(
        matches!(status, HookTrustStatus::NeedsTrust { .. }),
        "Modified hooks should need re-trust"
    );
}

#[test]
#[serial]
fn test_hook_re_trust_after_change() {
    use agent_of_empires::session::repo_config::{check_hook_trust, trust_repo, HookTrustStatus};

    let temp_home = TempDir::new().unwrap();
    setup_temp_home(temp_home.path());

    let repo = setup_repo_config(
        r#"
[hooks]
on_create = ["echo v1"]
"#,
    );

    // Trust v1
    let status = check_hook_trust(repo.path()).unwrap();
    if let HookTrustStatus::NeedsTrust { hooks_hash, .. } = &status {
        trust_repo(repo.path(), hooks_hash).unwrap();
    }

    // Modify to v2
    let aoe_dir = repo.path().join(".aoe");
    fs::write(
        aoe_dir.join("config.toml"),
        r#"
[hooks]
on_create = ["echo v2"]
"#,
    )
    .unwrap();

    // Re-trust v2
    let status = check_hook_trust(repo.path()).unwrap();
    assert!(matches!(status, HookTrustStatus::NeedsTrust { .. }));
    if let HookTrustStatus::NeedsTrust { hooks_hash, .. } = &status {
        trust_repo(repo.path(), hooks_hash).unwrap();
    }

    // Should now be trusted again
    let status = check_hook_trust(repo.path()).unwrap();
    assert!(
        matches!(status, HookTrustStatus::Trusted(_)),
        "Re-trusted hooks should be trusted"
    );
}
