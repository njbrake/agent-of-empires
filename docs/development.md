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
AGENT_OF_EMPIRES_DEBUG=1 cargo run  # With debug logging
RUST_LOG=agent_of_empires=debug cargo run  # With env_logger debug output
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
# Install VHS
brew install vhs

# Clear the demo profile
rm -rf ~/.agent-of-empires/profiles/demo

# Ensure demo directories exist
mkdir -p /tmp/demo-projects/api-server /tmp/demo-projects/web-app

# Generate the GIF (from repo root)
vhs assets/demo.tape
```

This creates `docs/assets/demo.gif`. The demo uses `-p demo` to run in a separate profile.
