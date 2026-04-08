use std::path::Path;
use std::process::Command;

fn main() {
    let web_dist = Path::new("web/dist");

    // Only rebuild frontend if dist/ is missing or source files changed
    println!("cargo:rerun-if-changed=web/src");
    println!("cargo:rerun-if-changed=web/index.html");
    println!("cargo:rerun-if-changed=web/package.json");
    println!("cargo:rerun-if-changed=web/vite.config.ts");
    println!("cargo:rerun-if-changed=web/tsconfig.json");

    if web_dist.exists() && web_dist.join("index.html").exists() {
        // Frontend already built
        return;
    }

    eprintln!("Building web frontend...");

    // Try bun first, fall back to npm
    let (cmd, install_args, build_args) = if Command::new("bun").arg("--version").output().is_ok() {
        ("bun", vec!["install"], vec!["run", "build"])
    } else if Command::new("npm").arg("--version").output().is_ok() {
        ("npm", vec!["install"], vec!["run", "build"])
    } else {
        // No JS runtime available -- create a minimal placeholder so compilation succeeds
        eprintln!(
            "WARNING: Neither bun nor npm found. Creating placeholder web/dist/. \
             Install bun or npm and run `cd web && bun run build` for the real dashboard."
        );
        std::fs::create_dir_all(web_dist).expect("Failed to create web/dist/");
        std::fs::write(
            web_dist.join("index.html"),
            "<html><body><h1>Dashboard not built</h1>\
             <p>Run <code>cd web && bun install && bun run build</code> then rebuild.</p>\
             </body></html>",
        )
        .expect("Failed to write placeholder");
        return;
    };

    let status = Command::new(cmd)
        .args(&install_args)
        .current_dir("web")
        .status()
        .expect("Failed to run package install");

    if !status.success() {
        panic!(
            "Web frontend install failed. Run `cd web && {} {}` manually.",
            cmd,
            install_args.join(" ")
        );
    }

    let status = Command::new(cmd)
        .args(&build_args)
        .current_dir("web")
        .status()
        .expect("Failed to run frontend build");

    if !status.success() {
        panic!(
            "Web frontend build failed. Run `cd web && {} {}` manually.",
            cmd,
            build_args.join(" ")
        );
    }
}
