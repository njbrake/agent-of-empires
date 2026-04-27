//! Self-update: detect install method, perform update.

use anyhow::{Context, Result};
use std::io::{ErrorKind, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

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

/// Download a release tarball to `dest`. Streams bytes; reports
/// progress via the optional callback (current bytes, total bytes
/// if known).
pub async fn download_tarball(
    url: &str,
    dest: &Path,
    mut on_progress: Option<&mut dyn FnMut(u64, Option<u64>)>,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("agent-of-empires")
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("download failed: HTTP {} from {}", response.status(), url);
    }
    let total = response.content_length();
    let mut stream = response.bytes_stream();
    let mut file = std::fs::File::create(dest)
        .with_context(|| format!("creating download file at {}", dest.display()))?;
    let mut downloaded: u64 = 0;
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        downloaded += chunk.len() as u64;
        if let Some(cb) = on_progress.as_deref_mut() {
            cb(downloaded, total);
        }
    }
    file.sync_all()?;
    Ok(())
}

/// Extract a `.tar.gz` into `dest_dir`. Shells out to `tar xzf`, which is
/// universally available on macOS/Linux and matches what `scripts/install.sh`
/// does. Returns the path to the extracted binary
/// (`dest_dir/aoe-{platform}`).
pub fn extract_tarball(tarball: &Path, dest_dir: &Path, platform: &str) -> Result<PathBuf> {
    let status = Command::new("tar")
        .arg("xzf")
        .arg(tarball)
        .arg("-C")
        .arg(dest_dir)
        .status()
        .context("running `tar xzf`")?;
    if !status.success() {
        anyhow::bail!("tar extraction failed (exit {})", status);
    }
    let extracted = dest_dir.join(format!("aoe-{platform}"));
    if !extracted.exists() {
        anyhow::bail!("extracted tarball did not contain {}", extracted.display());
    }
    Ok(extracted)
}

/// Run the candidate binary with `--version` and confirm its output
/// contains the expected version string. Defends against corrupt
/// downloads and wrong-arch tarballs that downloaded successfully but
/// won't run.
pub fn sanity_check_binary(binary: &Path, expected_version: &str) -> Result<()> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .with_context(|| format!("running {} --version", binary.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "candidate binary failed --version: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let matched = stdout
        .split_whitespace()
        .any(|tok| tok == expected_version || tok.trim_start_matches('v') == expected_version);
    if !matched {
        anyhow::bail!(
            "candidate binary reports {:?}, expected version {:?}",
            stdout.trim(),
            expected_version
        );
    }
    Ok(())
}

/// Atomically replace `target` with `source`. Both paths must be on the
/// same filesystem (callers ensure this by placing the temp file in the
/// same parent directory as the target). On `EACCES`, falls back to two
/// sequential `sudo` invocations (`sudo mv` then `sudo chmod 0755`); the
/// user gets one password prompt thanks to sudo's timestamp cache.
///
/// On Unix, this is safe to do while the target is the running binary -
/// the kernel keeps the old inode alive for the running process.
pub fn atomic_replace(source: &Path, target: &Path) -> Result<()> {
    match std::fs::rename(source, target) {
        Ok(()) => {
            #[cfg(unix)]
            std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755))?;
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::PermissionDenied => sudo_replace(source, target),
        Err(e) => Err(e).with_context(|| format!("renaming to {}", target.display())),
    }
}

fn sudo_replace(source: &Path, target: &Path) -> Result<()> {
    let mv_status = Command::new("sudo")
        .arg("mv")
        .arg(source)
        .arg(target)
        .status()
        .context("invoking `sudo mv`")?;
    if !mv_status.success() {
        anyhow::bail!("sudo mv failed (exit {})", mv_status);
    }
    let chmod_status = Command::new("sudo")
        .arg("chmod")
        .arg("0755")
        .arg(target)
        .status()
        .context("invoking `sudo chmod`")?;
    if !chmod_status.success() {
        anyhow::bail!("sudo chmod failed (exit {})", chmod_status);
    }
    Ok(())
}

/// Probe whether the parent directory of `binary_path` is writable
/// without sudo. Used by the confirm prompt to warn users before
/// they say yes.
pub fn parent_is_writable(binary_path: &Path) -> bool {
    let Some(parent) = binary_path.parent() else {
        return false;
    };
    let probe = parent.join(".aoe-update-writability-probe");
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Perform an in-place tarball update at `binary_path`, fetching the
/// release for `version`. Caller has already detected the install
/// method and confirmed with the user.
pub async fn update_via_tarball(
    binary_path: &Path,
    version: &str,
    on_progress: Option<&mut dyn FnMut(u64, Option<u64>)>,
) -> Result<()> {
    let platform = current_platform_string()?;
    let parent = binary_path
        .parent()
        .context("binary path has no parent directory")?;

    // Same-filesystem temp dir so the rename in atomic_replace works.
    let workdir = TempDir::new_in(parent).context("creating temp dir for update")?;

    let tarball_path = workdir.path().join(format!("aoe-{platform}.tar.gz"));
    let url = release_tarball_url(version, platform);
    download_tarball(&url, &tarball_path, on_progress).await?;

    let extracted = extract_tarball(&tarball_path, workdir.path(), platform)?;
    sanity_check_binary(&extracted, version)?;
    atomic_replace(&extracted, binary_path)?;
    Ok(())
}

pub fn update_via_brew() -> Result<()> {
    let status = Command::new("brew")
        .args(["update"])
        .status()
        .context("running `brew update`")?;
    if !status.success() {
        anyhow::bail!("`brew update` failed (exit {})", status);
    }
    let status = Command::new("brew")
        .args(["upgrade", "aoe"])
        .status()
        .context("running `brew upgrade aoe`")?;
    if !status.success() {
        anyhow::bail!("`brew upgrade aoe` failed (exit {})", status);
    }
    Ok(())
}

pub fn print_nix_refusal() {
    println!(
        "aoe was installed via Nix. Update by running:\n\
         \n    nix run github:njbrake/agent-of-empires\n\
         \n(or rebuild your flake input)."
    );
}

pub fn print_cargo_refusal() {
    println!(
        "aoe was installed via cargo. Update by running:\n\
         \n    cargo install --git https://github.com/njbrake/agent-of-empires aoe\n\
         \n(or `git pull && cargo install --path .` from a local clone)."
    );
}

pub fn print_unknown_refusal(binary_path: &Path) {
    println!(
        "Couldn't determine how aoe was installed at {}.\n\
         Reinstall with:\n\
         \n    curl -fsSL https://raw.githubusercontent.com/njbrake/agent-of-empires/main/scripts/install.sh | bash\n",
        binary_path.display()
    );
    let _ = std::io::stdout().flush();
}

/// Render the four-line confirm-prompt block. Used by both the CLI and
/// the TUI dialog. Produces no trailing newline; caller adds the
/// "Proceed? [Y/n]" line.
pub fn format_prompt_block(
    current_version: &str,
    latest_version: &str,
    method: &InstallMethod,
    needs_sudo: bool,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("Update v{current_version} → v{latest_version}\n"));
    let (method_label, location_label) = match method {
        InstallMethod::Homebrew => ("homebrew", "managed by Homebrew".to_string()),
        InstallMethod::Tarball { binary_path } => {
            ("tarball install", binary_path.display().to_string())
        }
        InstallMethod::Nix => ("nix", "/nix/store (read-only)".to_string()),
        InstallMethod::Cargo => ("cargo", "~/.cargo/bin/aoe".to_string()),
        InstallMethod::Unknown { binary_path } => ("unknown", binary_path.display().to_string()),
    };
    out.push_str(&format!("  Method:    {method_label}\n"));
    out.push_str(&format!("  Location:  {location_label}"));
    if needs_sudo {
        out.push_str("\n  Sudo:      required (write-protected directory)");
    }
    out
}

/// Top-level dispatch. The caller has already chosen the version and
/// done the user confirmation. `on_progress` is forwarded to the
/// tarball downloader (other paths ignore it).
pub async fn perform_update(
    method: &InstallMethod,
    version: &str,
    on_progress: Option<&mut dyn FnMut(u64, Option<u64>)>,
) -> Result<()> {
    match method {
        InstallMethod::Homebrew => update_via_brew(),
        InstallMethod::Tarball { binary_path } => {
            update_via_tarball(binary_path, version, on_progress).await
        }
        InstallMethod::Nix => {
            print_nix_refusal();
            Ok(())
        }
        InstallMethod::Cargo => {
            print_cargo_refusal();
            Ok(())
        }
        InstallMethod::Unknown { binary_path } => {
            print_unknown_refusal(binary_path);
            Ok(())
        }
    }
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

    #[test]
    fn prompt_block_tarball_no_sudo() {
        let m = InstallMethod::Tarball {
            binary_path: PathBuf::from("/home/u/.local/bin/aoe"),
        };
        let s = format_prompt_block("0.4.5", "0.5.0", &m, false);
        assert!(s.contains("Update v0.4.5 → v0.5.0"));
        assert!(s.contains("Method:    tarball install"));
        assert!(s.contains("Location:  /home/u/.local/bin/aoe"));
        assert!(!s.contains("Sudo:"));
    }

    #[test]
    fn prompt_block_tarball_sudo_required() {
        let m = InstallMethod::Tarball {
            binary_path: PathBuf::from("/usr/local/bin/aoe"),
        };
        let s = format_prompt_block("0.4.5", "0.5.0", &m, true);
        assert!(s.contains("Sudo:      required (write-protected directory)"));
    }

    #[test]
    fn prompt_block_homebrew_omits_location_path() {
        let s = format_prompt_block("0.4.5", "0.5.0", &InstallMethod::Homebrew, false);
        assert!(s.contains("Method:    homebrew"));
        assert!(s.contains("Location:  managed by Homebrew"));
    }

    #[test]
    fn prompt_block_nix() {
        let s = format_prompt_block("0.4.5", "0.5.0", &InstallMethod::Nix, false);
        assert!(s.contains("Method:    nix"));
    }
}
