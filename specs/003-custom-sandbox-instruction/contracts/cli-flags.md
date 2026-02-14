# CLI Flag Contracts: Custom Sandbox Instruction

This feature injects CLI flags into agent launch commands. No external API contracts are needed. The "contracts" are the agent CLI flag formats:

## Claude Code

```
claude --append-system-prompt "<escaped_instruction_text>"
```

- Flag: `--append-system-prompt`
- Argument: Double-quoted, shell-escaped string
- Behavior: Appends to default system prompt (does not replace)
- Modes: Works in both interactive and print mode

## OpenAI Codex

```
codex --config developer_instructions="<escaped_instruction_text>"
```

- Flag: `--config developer_instructions=`
- Argument: Inline key=value with double-quoted, shell-escaped string
- Behavior: Overrides developer instructions config value

## Unsupported Agents

Gemini, Vibe, OpenCode, and custom commands do not support CLI-based instruction injection. These trigger a warning popup and launch without the instruction.
