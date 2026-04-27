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
}
