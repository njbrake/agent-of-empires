# Config Schema Contracts

This feature does not expose HTTP/RPC APIs. The contracts are TOML config
file schemas that define the data format at each level.

## Global Config (`config.toml`)

New `[hooks]` section added to existing config:

```toml
# Existing sections remain unchanged...

[hooks]
on_create = ["echo 'setting up'", "npm install"]
on_launch = ["echo 'launching'"]
```

**Serialization rules**:
- `#[serde(default)]` on the `hooks` field: missing section = empty lists
- Empty lists are the default; no section written if both are empty

## Profile Config (`profiles/{name}/config.toml`)

New optional `[hooks]` section:

```toml
# Only fields that differ from global are present

[hooks]
on_create = ["pip install -r requirements.txt"]
# on_launch omitted = inherits from global
```

**Serialization rules**:
- `#[serde(default, skip_serializing_if = "Option::is_none")]` on
  the `hooks` field
- Each sub-field is `Option<Vec<String>>`; None = inherit from global
- If all sub-fields are None, the entire section is omitted

## Repo Config (`.aoe/config.toml`)

Existing schema, unchanged:

```toml
[hooks]
on_create = ["npm install", "cp .env.example .env"]
on_launch = ["npm install"]
```

**No changes to repo config format.** The Repo tab reads/writes this
existing format.

## Resolution Examples

### Example 1: Global only

```
Global:    on_create=["npm install"], on_launch=["echo hi"]
Profile:   (none)
Repo:      (none)
Resolved:  on_create=["npm install"], on_launch=["echo hi"]
```

### Example 2: Profile overrides on_create

```
Global:    on_create=["npm install"], on_launch=["echo hi"]
Profile:   on_create=["pip install -r req.txt"], on_launch=(none)
Repo:      (none)
Resolved:  on_create=["pip install -r req.txt"], on_launch=["echo hi"]
```

### Example 3: Repo overrides everything

```
Global:    on_create=["npm install"], on_launch=["echo hi"]
Profile:   on_create=["pip install"], on_launch=(none)
Repo:      on_create=["cargo build"], on_launch=[]
Resolved:  on_create=["cargo build"], on_launch=[]
```

Note: Repo explicitly setting `on_launch=[]` means "no launch hooks"
(overrides the global/profile value).

## Execution Context Contract

Hooks execute in the environment determined by the session's sandbox
setting, NOT by their config level origin.

### Non-sandboxed session

```
Execution: bash -c "{command}" in project_path
Working dir: /path/to/project
Environment: host OS
```

### Sandboxed session

```
Execution: docker exec --workdir {container_workdir} {container_name} bash -c "{command}"
Working dir: /workspace/{session_title} (inside container)
Environment: Docker container
```

### Failure contract

```
on_create failure:
  - Result: Session creation aborted
  - Cleanup: Worktree removed (if created)
  - User feedback: Error message displayed

on_launch failure:
  - Result: Session starts normally
  - User feedback: Warning logged
  - No cleanup needed
```

### Trust contract

```
Global hooks:  Always trusted (no dialog)
Profile hooks: Always trusted (no dialog)
Repo hooks:    Trust dialog on first use or config change
               Hash-based change detection (SHA-256)
               User can approve or skip
               If skipped: global/profile hooks apply instead
```
