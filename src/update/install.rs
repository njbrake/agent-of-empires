//! Self-update: detect install method, perform update.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallMethod {
    Homebrew,
    Tarball { binary_path: PathBuf },
    Nix,
    Cargo,
    Unknown { binary_path: PathBuf },
}

/// Pure prefix-based classification used by `detect_install_method`.
/// Returns the method as far as path prefixes can determine; Homebrew
/// detection requires running `brew list` and is layered on by
/// `classify_with_brew`.
pub fn classify_path_prefix(binary_path: &Path, home: &Path) -> InstallMethod {
    let path = binary_path;

    if path.starts_with("/nix/store/") {
        return InstallMethod::Nix;
    }

    let cargo_bin = home.join(".cargo").join("bin");
    if path.starts_with(&cargo_bin) {
        return InstallMethod::Cargo;
    }

    let known_bin_locations: [PathBuf; 3] = [
        PathBuf::from("/usr/local/bin"),
        home.join(".local").join("bin"),
        home.join("bin"),
    ];
    let parent = path.parent();
    if parent.is_some_and(|p| known_bin_locations.iter().any(|k| p == k.as_path())) {
        return InstallMethod::Tarball {
            binary_path: path.to_path_buf(),
        };
    }

    InstallMethod::Unknown {
        binary_path: path.to_path_buf(),
    }
}

/// Layer Homebrew detection on top of the prefix classification:
/// only return `Homebrew` if `brew list aoe` produced a path that
/// canonicalizes to the same file as the running binary. Otherwise
/// keep the prefix classification.
pub fn classify_with_brew(
    prefix: InstallMethod,
    brew_path: Option<&Path>,
    binary_path: &Path,
) -> InstallMethod {
    if let Some(bp) = brew_path {
        if paths_canonicalize_equal(bp, binary_path) {
            return InstallMethod::Homebrew;
        }
    }
    prefix
}

fn paths_canonicalize_equal(a: &Path, b: &Path) -> bool {
    let a_canon = a.canonicalize().ok();
    let b_canon = b.canonicalize().ok();
    match (a_canon, b_canon) {
        (Some(a), Some(b)) => a == b,
        _ => a == b, // fall back to literal equality if canonicalize fails
    }
}

pub fn detect_install_method() -> Result<InstallMethod> {
    let exe = std::env::current_exe().context("locating current executable")?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let home = dirs::home_dir().context("locating home directory")?;
    let prefix = classify_path_prefix(&exe, &home);
    let brew_path = probe_brew_aoe_path();
    Ok(classify_with_brew(prefix, brew_path.as_deref(), &exe))
}

/// Run `brew list aoe` and return the path to the installed binary, if any.
/// We parse the output (one path per line) and pick the line that ends in
/// `/aoe` or `/bin/aoe`. If brew is not installed, the formula is not installed,
/// or the command fails for any other reason, return `None`.
fn probe_brew_aoe_path() -> Option<PathBuf> {
    let output = Command::new("brew").args(["list", "aoe"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with("/aoe") || trimmed.ends_with("/bin/aoe") {
            return Some(PathBuf::from(trimmed));
        }
    }
    None
}

/// Return the platform string used in release tarball asset names
/// (e.g. `linux-amd64`). `os` matches `std::env::consts::OS`,
/// `arch` matches `std::env::consts::ARCH`.
pub fn platform_string_for(os: &str, arch: &str) -> Result<&'static str> {
    let os_norm = match os {
        "linux" => "linux",
        "macos" => "darwin",
        other => anyhow::bail!("unsupported OS: {other}"),
    };
    let arch_norm = match arch {
        "x86_64" => "amd64",
        "aarch64" | "arm64" => "arm64",
        other => anyhow::bail!("unsupported architecture: {other}"),
    };
    // Static lookup so we can return &'static str.
    Ok(match (os_norm, arch_norm) {
        ("linux", "amd64") => "linux-amd64",
        ("linux", "arm64") => "linux-arm64",
        ("darwin", "amd64") => "darwin-amd64",
        ("darwin", "arm64") => "darwin-arm64",
        _ => unreachable!(),
    })
}

pub fn current_platform_string() -> Result<&'static str> {
    platform_string_for(std::env::consts::OS, std::env::consts::ARCH)
}

pub fn release_tarball_url(version: &str, platform: &str) -> String {
    format!(
        "https://github.com/njbrake/agent-of-empires/releases/download/v{version}/aoe-{platform}.tar.gz"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/kevin")
    }

    #[test]
    fn classifies_nix_store() {
        let p = PathBuf::from("/nix/store/abc123-aoe-0.4.5/bin/aoe");
        assert_eq!(classify_path_prefix(&p, &home()), InstallMethod::Nix);
    }

    #[test]
    fn classifies_cargo_bin() {
        let p = home().join(".cargo/bin/aoe");
        assert_eq!(classify_path_prefix(&p, &home()), InstallMethod::Cargo);
    }

    #[test]
    fn classifies_usr_local_bin_as_tarball() {
        let p = PathBuf::from("/usr/local/bin/aoe");
        assert_eq!(
            classify_path_prefix(&p, &home()),
            InstallMethod::Tarball { binary_path: p }
        );
    }

    #[test]
    fn classifies_local_bin_as_tarball() {
        let p = home().join(".local/bin/aoe");
        assert_eq!(
            classify_path_prefix(&p, &home()),
            InstallMethod::Tarball {
                binary_path: p.clone()
            }
        );
    }

    #[test]
    fn classifies_home_bin_as_tarball() {
        let p = home().join("bin/aoe");
        assert_eq!(
            classify_path_prefix(&p, &home()),
            InstallMethod::Tarball {
                binary_path: p.clone()
            }
        );
    }

    #[test]
    fn classifies_random_path_as_unknown() {
        let p = PathBuf::from("/opt/aoe-custom/bin/aoe");
        assert_eq!(
            classify_path_prefix(&p, &home()),
            InstallMethod::Unknown { binary_path: p }
        );
    }

    #[test]
    fn brew_takes_priority_when_paths_match() {
        // brew probe returned a path that canonicalizes to the running binary
        let exe = PathBuf::from("/opt/homebrew/Cellar/aoe/0.4.5/bin/aoe");
        let brew_path = Some(exe.clone());
        let prefix_class = InstallMethod::Unknown {
            binary_path: exe.clone(),
        };
        let result = classify_with_brew(prefix_class, brew_path.as_deref(), &exe);
        assert_eq!(result, InstallMethod::Homebrew);
    }

    #[test]
    fn brew_ignored_when_paths_differ() {
        // brew is installed (probe returned a path) but the running binary
        // is somewhere else - keep the prefix classification
        let exe = PathBuf::from("/usr/local/bin/aoe");
        let brew_path = Some(PathBuf::from("/opt/homebrew/Cellar/aoe/0.4.5/bin/aoe"));
        let prefix_class = InstallMethod::Tarball {
            binary_path: exe.clone(),
        };
        let result = classify_with_brew(prefix_class.clone(), brew_path.as_deref(), &exe);
        assert_eq!(result, prefix_class);
    }

    #[test]
    fn brew_ignored_when_probe_returned_none() {
        let exe = PathBuf::from("/usr/local/bin/aoe");
        let prefix_class = InstallMethod::Tarball {
            binary_path: exe.clone(),
        };
        let result = classify_with_brew(prefix_class.clone(), None, &exe);
        assert_eq!(result, prefix_class);
    }

    #[test]
    fn platform_string_linux_x86_64() {
        assert_eq!(
            platform_string_for("linux", "x86_64").unwrap(),
            "linux-amd64"
        );
    }

    #[test]
    fn platform_string_linux_aarch64() {
        assert_eq!(
            platform_string_for("linux", "aarch64").unwrap(),
            "linux-arm64"
        );
    }

    #[test]
    fn platform_string_macos_amd64() {
        assert_eq!(
            platform_string_for("macos", "x86_64").unwrap(),
            "darwin-amd64"
        );
    }

    #[test]
    fn platform_string_macos_arm64() {
        assert_eq!(
            platform_string_for("macos", "aarch64").unwrap(),
            "darwin-arm64"
        );
    }

    #[test]
    fn platform_string_unsupported_arch_errors() {
        let err = platform_string_for("linux", "riscv64").unwrap_err();
        assert!(err.to_string().contains("riscv64"));
    }

    #[test]
    fn platform_string_unsupported_os_errors() {
        let err = platform_string_for("windows", "x86_64").unwrap_err();
        assert!(err.to_string().contains("windows"));
    }

    #[test]
    fn release_tarball_url_format() {
        let url = release_tarball_url("0.5.0", "linux-amd64");
        assert_eq!(
            url,
            "https://github.com/njbrake/agent-of-empires/releases/download/v0.5.0/aoe-linux-amd64.tar.gz"
        );
    }
}
