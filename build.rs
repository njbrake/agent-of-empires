fn main() {
    check_stale_build_cache();

    #[cfg(feature = "serve")]
    build_frontend();
}

/// Detect stale build caches by tracking Cargo.lock content hash.
///
/// When Cargo.lock changes (dependency updates, feature additions, branch
/// switches in worktrees), the target/ directory can contain incompatible
/// artifacts that cause cryptic compilation errors like "can't find crate"
/// or "found possibly newer version of crate." This check catches that
/// early with a clear message instead of letting the build fail inscrutably.
fn check_stale_build_cache() {
    use std::path::Path;

    // Re-run this check whenever Cargo.lock changes.
    println!("cargo:rerun-if-changed=Cargo.lock");

    let lockfile = Path::new("Cargo.lock");
    let target_dir = std::env::var("OUT_DIR")
        .ok()
        .and_then(|out| {
            // OUT_DIR is something like target/debug/build/agent-of-empires-xxx/out
            // Walk up to find the target/ root.
            let mut p = Path::new(&out).to_path_buf();
            while p.pop() {
                if p.file_name().is_some_and(|n| n == "target") {
                    return Some(p);
                }
            }
            None
        })
        .unwrap_or_else(|| Path::new("target").to_path_buf());

    let hash_file = target_dir.join(".cargo-lock-hash");

    let Ok(lock_content) = std::fs::read(lockfile) else {
        return; // No Cargo.lock, nothing to check.
    };

    // Simple, fast hash: use the file length + first/last 1KB as a fingerprint.
    // This avoids pulling in a hash crate in build.rs.
    let len = lock_content.len();
    let head: u64 = lock_content[..len.min(1024)]
        .iter()
        .fold(0u64, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u64));
    let tail: u64 = lock_content[len.saturating_sub(1024)..]
        .iter()
        .fold(0u64, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u64));
    let current_hash = format!("{:x}{:x}{:x}", len, head, tail);

    if let Ok(stored_hash) = std::fs::read_to_string(&hash_file) {
        if stored_hash.trim() != current_hash {
            println!(
                "cargo:warning=Cargo.lock changed since last build. \
                 If you see strange compilation errors, run `cargo clean`."
            );
        }
    }

    // Always update the stored hash.
    let _ = std::fs::write(&hash_file, &current_hash);
}

#[cfg(feature = "serve")]
fn build_frontend() {
    use std::path::Path;
    use std::process::Command;

    println!("cargo:rerun-if-changed=web/src");
    println!("cargo:rerun-if-changed=web/index.html");
    println!("cargo:rerun-if-changed=web/package.json");
    println!("cargo:rerun-if-changed=web/package-lock.json");
    println!("cargo:rerun-if-changed=web/vite.config.ts");
    println!("cargo:rerun-if-changed=web/tsconfig.json");

    // AOE_WEB_DIST allows Nix (and other reproducible build systems) to supply
    // a pre-built frontend directory, bypassing the npm build entirely. When
    // set, the directory is copied to web/dist/ and npm is not invoked.
    //
    // Registered unconditionally so Cargo re-runs build.rs when the var is
    // added or removed, not only when it is already set.
    println!("cargo:rerun-if-env-changed=AOE_WEB_DIST");
    if let Ok(dist_src) = std::env::var("AOE_WEB_DIST") {
        eprintln!("Using pre-built web frontend from AOE_WEB_DIST={dist_src}");
        let src = Path::new(&dist_src);
        let dst = Path::new("web/dist");
        if dst.exists() {
            std::fs::remove_dir_all(dst).expect("Failed to remove existing web/dist");
        }
        // Recursively copy src -> web/dist
        copy_dir(src, dst);
        return;
    }

    // Always rebuild: the rerun-if-changed directives above ensure this
    // function only runs when web source files actually changed.
    // Previously this short-circuited when dist/ existed, which meant
    // source changes were silently ignored.

    eprintln!("Building web frontend...");

    assert!(
        Command::new("npm").arg("--version").output().is_ok(),
        "npm is required to build with --features serve. Install Node.js: https://nodejs.org/"
    );

    maybe_install_web_deps();

    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir("web")
        .status()
        .expect("Failed to run npm run build");

    if !status.success() {
        panic!("npm run build failed in web/. Run `cd web && npm run build` to debug.");
    }
}

/// Install web dependencies when node_modules is missing OR stale relative to
/// package.json / package-lock.json.
///
/// The previous check only looked for `web/node_modules/.package-lock.json` and
/// skipped install when it existed. That broke a real workflow: after pulling
/// new commits that add a dependency (e.g. `cmdk`), contributors hit cryptic
/// TypeScript errors like "Cannot find module 'cmdk'" because the old
/// node_modules was considered "good enough." This now compares mtimes so any
/// lockfile change triggers a reinstall.
#[cfg(feature = "serve")]
fn maybe_install_web_deps() {
    use std::path::Path;
    use std::process::Command;

    let node_modules_marker = Path::new("web/node_modules/.package-lock.json");
    let package_json = Path::new("web/package.json");
    let package_lock = Path::new("web/package-lock.json");

    let marker_mtime = node_modules_marker
        .metadata()
        .and_then(|m| m.modified())
        .ok();
    let stale = match marker_mtime {
        None => true, // fresh clone, no node_modules yet
        Some(marker) => is_newer_than(package_json, marker) || is_newer_than(package_lock, marker),
    };

    if !stale {
        return;
    }

    // Prefer `npm ci` when a lockfile exists: it is deterministic and cleans
    // up drift from manual edits. Fall back to `npm install` for projects
    // without a lockfile (unusual, but keeps first-time setup working).
    let install_cmd = if package_lock.exists() {
        "ci"
    } else {
        "install"
    };

    // Use `cargo:warning=` so the notice shows in a default `cargo build`
    // (plain eprintln! is suppressed unless the user passes -vv).
    println!(
        "cargo:warning=Installing web dependencies via `npm {install_cmd}` (node_modules is stale or missing)..."
    );

    let status = Command::new("npm")
        .args([install_cmd])
        .current_dir("web")
        .status()
        .unwrap_or_else(|e| panic!("Failed to spawn `npm {install_cmd}` in web/: {e}"));

    if !status.success() {
        panic!(
            "`npm {install_cmd}` failed in web/. \
             Run `cd web && npm {install_cmd}` to see the full error."
        );
    }
}

#[cfg(feature = "serve")]
fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).expect("Failed to create directory");
    for entry in std::fs::read_dir(src).expect("Failed to read directory") {
        let entry = entry.expect("Failed to read entry");
        let dst_path = dst.join(entry.file_name());
        if entry.file_type().expect("Failed to get file type").is_dir() {
            copy_dir(&entry.path(), &dst_path);
        } else {
            std::fs::copy(entry.path(), dst_path).expect("Failed to copy file");
        }
    }
}

#[cfg(feature = "serve")]
fn is_newer_than(path: &std::path::Path, reference: std::time::SystemTime) -> bool {
    match path.metadata().and_then(|m| m.modified()) {
        Ok(mtime) => mtime > reference,
        Err(_) => false, // if the file doesn't exist, it can't be newer
    }
}
