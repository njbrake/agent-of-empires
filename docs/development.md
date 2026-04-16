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

## Generating the Web Dashboard GIFs

`docs/assets/web-desktop.gif` and `docs/assets/web-mobile.gif` are recorded against a real `aoe serve` backend with real opencode sessions, no mocks. The recorder lives in `web/scripts/record-web-demo.mjs`.

```bash
# 1. Build with the serve feature.
cargo build --release --features serve

# 2. Set up an isolated profile with two scratch git repos and two opencode sessions.
SANDBOX=/tmp/aoe-webdemo
rm -rf "$SANDBOX"
mkdir -p "$SANDBOX/home/.config" "$SANDBOX/projects/api-server" "$SANDBOX/projects/web-app"
for d in "$SANDBOX/projects/"*; do
  (cd "$d" && git init -q && git config user.email t@t && git config user.name t \
    && touch README.md && git add . && git commit -q -m init)
done
export HOME=$SANDBOX/home XDG_CONFIG_HOME=$SANDBOX/home/.config
target/release/aoe add "$SANDBOX/projects/api-server" -t "API Server" -c opencode
target/release/aoe add "$SANDBOX/projects/web-app"    -t "Web App"    -c opencode

# 3. Start the server (no auth, localhost only).
target/release/aoe serve --host 127.0.0.1 --port 8181 --no-auth &

# 4. Record both viewports. Each run drives the live dashboard with Playwright,
#    captures WebM, and converts to GIF with ffmpeg.
node web/scripts/record-web-demo.mjs --viewport desktop --port 8181
node web/scripts/record-web-demo.mjs --viewport mobile  --port 8181
```

opencode's free tier needs no credentials, so the sessions produce real LLM responses inside the recording. Reset between runs by killing tmux (`HOME=$SANDBOX/home tmux kill-server`) so each session starts fresh.
