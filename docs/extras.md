# Extras

Beyond managing AI coding agents, AoE has a few fun extras built in.

## settl -- Play Settlers of Catan

[settl](https://github.com/mozilla-ai/settl) is a terminal Settlers of Catan game with LLM-powered AI players. It's built with ratatui (same as AoE) and supports Claude, GPT, Gemini, and local models via llamafile.

Select **settl** from the tool picker when creating a new session to launch a game right inside AoE. Watch AI players trade, build, and compete -- or jump in yourself with `--human` mode.

### Setup

1. Install settl ([instructions](https://github.com/mozilla-ai/settl#quickstart))
2. Create a new session in AoE and pick **settl** as the tool
3. Optionally pass flags like `--players 3` or `--model claude-sonnet-4-6` via the extra args field

settl is a host-only launch -- sandbox and worktree options are automatically hidden since it's a game, not a coding agent.

### Status Detection

AoE integrates with settl's [hook system](https://github.com/mozilla-ai/settl#hooks) for status detection. When you launch settl through AoE, hooks are automatically installed in `~/.settl/config.toml` that report game state back to the dashboard:

- **Running** -- an AI player is taking their turn
- **Waiting** -- it's your turn (human mode)
- **Idle** -- game over

## Sound Effects

AoE can play RPG sound effects when agent sessions change state. Hear a spell cast when a session starts, a blade swing when an agent is working, or a roar when something errors out.

See the full [Sound Effects](/docs/sounds/) guide for setup, configuration, and custom sound support.
