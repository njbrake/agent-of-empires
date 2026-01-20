# Development


## Generating the Demo GIF

The demo GIF in the README is created using [VHS](https://github.com/charmbracelet/vhs), a tool that generates terminal GIFs from scripts.

### Install VHS

```bash
brew install vhs
```

### Regenerate the GIF

The `assets/demo.tape` file defines the demo script. To regenerate:

```bash
# Clear the demo profile (so it starts fresh)
rm -rf ~/.agent-of-empires/profiles/demo

# Ensure demo directories exist
mkdir -p /tmp/demo-projects/api-server /tmp/demo-projects/web-app

# Generate the GIF (run from repo root)
vhs assets/demo.tape
```

This creates `docs/assets/demo.gif`.

The demo uses `-p demo` to run in a separate profile so it doesn't affect your real sessions.

See the [VHS documentation](https://github.com/charmbracelet/vhs) for more options.
