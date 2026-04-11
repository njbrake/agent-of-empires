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

    let web_dist = Path::new("web/dist");

    // Only rebuild frontend if dist/ is missing or source files changed
    println!("cargo:rerun-if-changed=web/src");
    println!("cargo:rerun-if-changed=web/index.html");
    println!("cargo:rerun-if-changed=web/package.json");
    println!("cargo:rerun-if-changed=web/vite.config.ts");
    println!("cargo:rerun-if-changed=web/tsconfig.json");

    if web_dist.exists() && web_dist.join("index.html").exists() {
        return;
    }

    eprintln!("Building web frontend...");

    assert!(
        Command::new("npm").arg("--version").output().is_ok(),
        "npm is required to build with --features serve. Install Node.js: https://nodejs.org/"
    );

    let status = Command::new("npm")
        .args(["install"])
        .current_dir("web")
        .status()
        .expect("Failed to run npm install");

    if !status.success() {
        panic!("npm install failed in web/. Run `cd web && npm install` to debug.");
    }

    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir("web")
        .status()
        .expect("Failed to run npm run build");

    if !status.success() {
        panic!("npm run build failed in web/. Run `cd web && npm run build` to debug.");
    }
}
