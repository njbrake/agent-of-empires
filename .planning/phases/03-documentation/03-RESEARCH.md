# Phase 3: Documentation - Research

**Researched:** 2026-02-08
**Domain:** mdBook documentation, configuration file documentation, environment variables
**Confidence:** HIGH

## Summary

This phase requires documenting the new profile environment variables feature added in Phase 1 and Phase 2. The documentation system uses mdBook v0.4.40 to build a static site from Markdown files in the `docs/` directory. The feature adds top-level `environment` and `environment_values` fields to ProfileConfig that merge with global sandbox environment settings and apply to both sandbox and non-sandbox modes.

Research examined:
- Current documentation structure and patterns in this project
- mdBook features and syntax used
- How existing configuration documentation is written
- The technical implementation of profile environment variables

**Primary recommendation:** Update `docs/guides/configuration.md` with a new Profile Environment Variables section following existing patterns, add use case examples, and consider creating a dedicated profile usage guide.

## Standard Stack

The project uses a straightforward documentation stack with no special dependencies.

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| **mdBook** | v0.4.40 | Static site generator from Markdown | Official Rust documentation tool, integrates with GitHub Pages |
| **CommonMark** | (mdBook default) | Markdown parsing | Standard Markdown syntax with GitHub-style extensions |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| **TOML syntax highlighting** | (mdBook default) | Configuration examples | All TOML code blocks should use `toml` language identifier |
| **Bash syntax highlighting** | (mdBook default) | Command examples | All shell commands should use `bash` language identifier |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| mdBook | Docusaurus, MkDocs, Hugo | mdBook is lighter, Rust-native, already configured; alternatives would require migration and are overkill for this project |

**Installation:**
```bash
# Install mdbook (if not already installed)
cargo install mdbook

# Build documentation
mdbook build

# Output goes to: book/ directory
```

## Architecture Patterns

### Recommended Documentation Structure
```
docs/
├── guides/              # Usage guides (the focus for this phase)
│   ├── configuration.md  # PRIMARY: Update this file
│   ├── sandbox.md       # Reference for sandbox examples
│   └── workflow.md     # Reference for profile usage patterns
├── cli/                # CLI reference (auto-generated)
├── assets/             # Images, GIFs for documentation
├── SUMMARY.md          # Table of contents (may need update)
└── index.md           # Introduction
```

### Pattern 1: Configuration Documentation Format
**What:** Consistent format for documenting configuration options
**When to use:** All configuration reference sections
**Example:**
```markdown
## Section Name

Brief description of what this configuration does.

```toml
[section_name]
option = "value"
another_option = true
```

| Option | Default | Description |
|--------|---------|-------------|
| `option` | (default value) | What this option does |
| `another_option` | `true` | Another option description |

### Subsection

Additional explanation or related options.
```

**Source:** Existing `docs/guides/configuration.md` lines 40-95

### Pattern 2: Code Block with Language Syntax
**What:** Always specify language for code blocks to enable syntax highlighting
**When to use:** All code blocks
**Example:**
```markdown
```toml
[sandbox]
environment = ["ANTHROPIC_API_KEY"]
```

```bash
aoe add --sandbox .
```
```

**Source:** Standard mdBook practice verified in existing docs

### Pattern 3: Use Cases with Examples
**What:** Provide real-world scenarios showing how to use a feature
**When to use:** New features or complex configuration options
**Example:**
```markdown
## Use Cases

### Scenario 1: Different API keys per client

For a consulting project, use profile environment variables to avoid exposing client secrets:

```toml
# ~/.agent-of-empires/profiles/client-a/config.toml
environment_values = { ANTHROPIC_API_KEY = "$CLIENT_A_KEY" }
```

```bash
export CLIENT_A_KEY="sk-ant-..."
aoe -p client-a
```
```

**Source:** Pattern from `docs/guides/sandbox.md` lines 125-149

### Anti-Patterns to Avoid
- **Missing language identifier:** Code blocks without language tags don't get syntax highlighting
- **Inconsistent table formatting:** Tables should align with existing documentation style
- **No real examples:** Avoid abstract descriptions; always provide concrete TOML snippets
- **Orphaned sections:** New sections should be cross-referenced from relevant parts of the docs

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Custom documentation generator | Write custom scripts to process Markdown | mdBook build | mdBook handles navigation, search, syntax highlighting, and HTML generation automatically |
| Manual table of contents | Manually maintain navigation links | SUMMARY.md auto-generates sidebar | mdBook's SUMMARY.md drives the entire navigation structure |
| Custom CSS for styling | Create theme from scratch | mdBook themes + existing custom.css | Project already has custom CSS in `theme/css/custom.css` |

**Key insight:** mdBook's built-in features are sufficient for this documentation. The project already has a working build pipeline (`scripts/build-site.sh`) that integrates mdBook with an Astro website. Don't introduce additional complexity.

## Common Pitfalls

### Pitfall 1: Inconsistent Option Documentation
**What goes wrong:** Configuration options are documented inconsistently (some with defaults, some without, different terminology)
**Why it happens:** When adding new sections, developers often copy-paste and forget to follow the established table format
**How to avoid:** Follow the exact table format from existing sections:
```markdown
| Option | Default | Description |
|--------|---------|-------------|
| `option_name` | `default_value` | Clear description |
```
**Warning signs:** Tables missing the "Default" column, or descriptions that are too brief

### Pitfall 2: Missing Cross-References
**What goes wrong:** New sections are added but not linked from related documentation
**Why it happens:** Focusing on a single file without considering the overall doc navigation
**How to avoid:** When adding a new major section, check:
- Does it need a link from SUMMARY.md?
- Should it be referenced from other guides?
- Is there a related configuration option in another section that should point here?
**Warning signs:** Readers can't find information through natural navigation paths

### Pitfall 3: Outdated Feature Descriptions
**What goes wrong:** Documentation describes old behavior after a feature change
**Why it happens:** Documentation isn't updated when code changes, or assumptions are made about behavior
**How to avoid:** Verify against actual code behavior:
1. Check the implementation in `src/session/profile_config.rs`
2. Verify merge logic in `merge_env_vars_with_profile()`
3. Test configuration examples if possible
**Warning signs:** Uncertainty about whether a feature works as documented

### Pitfall 4: Incorrect TOML Syntax in Examples
**What goes wrong:** Code examples have invalid TOML syntax that doesn't match the actual schema
**Why it happens:** Writing examples without checking the actual struct definitions or serialization logic
**How to avoid:** Reference the actual code:
- ProfileConfig structure in `src/session/profile_config.rs`
- How `skip_serializing_if = "Option::is_none"` affects output
- Default values from implementation
**Warning signs:** Examples that "look right" but aren't verified

## Code Examples

Verified patterns from existing documentation:

### Configuration Section Header
```markdown
## Environment Variables

Description of what these environment variables do.
```

**Source:** `docs/guides/configuration.md` line 33

### Configuration Table
```markdown
| Option | Default | Description |
|--------|---------|-------------|
| `AGENT_OF_EMPIRES_PROFILE` | (auto-detect) | Default profile to use |
```

**Source:** `docs/guides/configuration.md` lines 35-38

### TOML Configuration Example
```markdown
```toml
[sandbox]
environment = ["ANTHROPIC_API_KEY"]
environment_values = { GH_TOKEN = "$AOE_GH_TOKEN" }
```
```

**Source:** `docs/guides/configuration.md` lines 89-90

### Environment vs Environment Values Explanation
```markdown
### environment vs environment_values

- **`environment`** passes host env vars by name. The host value is read at container start.
- **`environment_values`** injects fixed values. Values starting with `$` reference a host env var (e.g., `"$AOE_GH_TOKEN"` reads `AOE_GH_TOKEN` from the host). Use `$$` for a literal `$`.
```

**Source:** `docs/guides/configuration.md` lines 111-114

### Profile Usage Example
```markdown
```bash
aoe -p work
aoe profile create client-xyz
aoe profile default work
```
```

**Source:** `docs/guides/configuration.md` lines 174-179

### Use Case Scenario
```markdown
### Use Case

Brief description of when to use this feature.

```toml
[profile.environment_values]
KEY = "value"
```

Explanation of what this does.
```

**Source:** Pattern from `docs/guides/sandbox.md` lines 125-149

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No profile env vars | Profile-level environment and environment_values | Phase 1 (recent) | Profiles can now customize environment per profile |
| Only sandbox env vars | Environment vars apply to both sandbox AND non-sandbox modes | Phase 1 | Unified environment configuration across session types |

**Deprecated/outdated:**
- Environment variables only in SandboxConfig: Profile-level env vars are now top-level in ProfileConfig

## Open Questions

1. **Should a dedicated profile guide be created?**
   - What we know: Current docs have profile info scattered between configuration.md and workflow.md
   - What's unclear: Whether a dedicated `docs/guides/profiles.md` would be better or cause redundancy
   - Recommendation: Update configuration.md first with comprehensive profile env var documentation. If content becomes too large, consider extracting to dedicated guide.

2. **How to handle TUI settings screen documentation?**
   - What we know: Every config field must be editable in TUI settings (per AGENTS.md)
   - What's unclear: Whether the TUI settings interface needs documentation updates
   - Recommendation: Check if the TUI settings screen needs documentation updates, or if configuration.md is sufficient.

3. **Should environment precedence be explicitly documented?**
   - What we know: Profile env vars override sandbox env vars on name conflicts
   - What's unclear: Whether this merge precedence behavior needs explicit documentation
   - Recommendation: Document the precedence rules clearly (profile > sandbox) to avoid user confusion.

## Sources

### Primary (HIGH confidence)
- **mdBook v0.4.40 official documentation** - Verified syntax, features, build process
- **Existing project docs** - `docs/guides/configuration.md`, `docs/guides/sandbox.md`, `docs/guides/workflow.md` - Established patterns and conventions
- **Source code** - `src/session/profile_config.rs`, `src/session/config.rs`, `src/session/instance.rs` - Feature implementation details

### Secondary (MEDIUM confidence)
- **book.toml** - Project's mdBook configuration
- **scripts/build-site.sh** - Documentation build pipeline
- **.github/workflows/docs.yml** - CI/CD documentation deployment

### Tertiary (LOW confidence)
- None required - All findings verified against project code and existing documentation

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - mdBook version confirmed from CI workflows, patterns verified in project
- Architecture: HIGH - Existing documentation structure analyzed, patterns identified from actual docs
- Pitfalls: HIGH - Based on common documentation problems and existing project code

**Research date:** 2026-02-08
**Valid until:** 30 days - mdBook is stable, project documentation patterns are consistent
