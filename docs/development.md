# Development

## Building

```bash
cargo build                    # Debug build
cargo build --release          # Release build (with LTO)
cargo build --profile dev-release  # Optimized build without LTO (faster compile)
```

The release binary is at `target/release/aoe`.

## Running

```bash
cargo run --release            # Run from source
AGENT_OF_EMPIRES_DEBUG=1 cargo run  # Debug logging (writes to debug.log in app data dir)
```

Requires `tmux` to be installed.

## Testing

```bash
cargo test       # Unit + integration tests
cargo fmt        # Format code
cargo clippy     # Lint
cargo check      # Fast type-check
```

Some integration tests require `tmux` to be available and will skip if it's not installed.

## Generating the Demo GIF

The demo GIF in the docs is created using [VHS](https://github.com/charmbracelet/vhs).

```bash
# Install VHS (macOS)
brew install vhs

# Build aoe with the serve feature so the tape can exercise remote access
cargo build --release --features serve

# Generate the GIF (from repo root). The tape cleans its own profile
# (`~/.config/agent-of-empires/profiles/demo` on Linux,
# `~/.agent-of-empires/profiles/demo` on macOS) and demo scratch repo.
vhs assets/demo.tape
```

This writes `docs/assets/demo.gif`. The tape runs `aoe -p demo` so your real profile is untouched.
