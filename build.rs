fn main() {
    #[cfg(feature = "serve")]
    build_frontend();
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
