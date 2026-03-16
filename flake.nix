{
  description = "Terminal session manager for AI coding agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    crane.url = "github:ipetkov/crane";
  };

  outputs = inputs @ { flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      perSystem = { config, self', inputs', pkgs, system, ... }:
        let
          craneLib = inputs.crane.mkLib pkgs;

          # git2 uses vendored-openssl (needs perl to build OpenSSL)
          # and libgit2-sys vendors libgit2 (needs cmake to build it)
          nativeBuildInputs = with pkgs; [
            pkg-config
            perl
            cmake
            installShellFiles
          ];

          buildInputs = with pkgs; [
            zlib # required by vendored libgit2
          ];

          commonArgs = {
            src = craneLib.cleanCargoSource ./.;
            strictDeps = true;
            inherit nativeBuildInputs buildInputs;
          };

          # Build only workspace dependencies first (for caching)
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          aoe = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            cargoExtraArgs = "--package agent-of-empires";
            doCheck = false;
            postInstall = ''
              installShellCompletion --cmd aoe \
                --bash <($out/bin/aoe completion bash) \
                --fish <($out/bin/aoe completion fish) \
                --zsh <($out/bin/aoe completion zsh)
            '';

            meta = with pkgs.lib; {
              description = "Terminal session manager for AI coding agents";
              longDescription = ''
                Agent of Empires (AoE) is a terminal session manager for AI coding
                agents on Linux and macOS. Built on tmux, it allows running multiple
                AI agents in parallel across different branches of your codebase,
                each in its own isolated session with optional Docker sandboxing.

                Supports Claude Code, OpenCode, Mistral Vibe, Codex CLI, and Gemini CLI.
              '';
              homepage = "https://github.com/njbrake/agent-of-empires";
              license = licenses.mit;
              platforms = platforms.unix;
              mainProgram = "aoe";
            };
          });
        in
        {
          packages.default = aoe;

          checks = {
            # Build the package as a check too
            inherit aoe;

            aoe-clippy = craneLib.cargoClippy (commonArgs // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--package agent-of-empires --all-targets -- --deny warnings";
            });

            aoe-fmt = craneLib.cargoFmt {
              inherit (commonArgs) src;
            };

            aoe-test = craneLib.cargoTest (commonArgs // {
              inherit cargoArtifacts;
              cargoTestExtraArgs = "--package agent-of-empires";
              # Some git:: unit tests invoke the git binary directly
              nativeBuildInputs = commonArgs.nativeBuildInputs ++ [ pkgs.git ];
            });
          };

          devShells.default = craneLib.devShell {
            checks = self'.checks;
            packages = with pkgs; [
              rust-analyzer
              tmux
            ];
          };
        };
    };
}
