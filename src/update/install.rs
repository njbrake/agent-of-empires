//! Self-update: detect install method, perform update.

use std::path::{Path, PathBuf};

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
}
