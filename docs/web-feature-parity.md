# Web UI Feature Parity Roadmap

This document maps every TUI feature to its web UI status and the work needed to close each gap.

The good news: **all the logic already exists in the Rust library.** The web UI just needs API endpoints (thin wrappers around existing functions) and React components. No new business logic required.

## Status Legend

- Done: shipped in the web UI today
- API only: needs a new endpoint in `src/server/api.rs`, then a React component
- Frontend only: backend already exposes enough data, just needs UI
- Not applicable: doesn't make sense in a browser context

## Feature Matrix

### Session Lifecycle

| Feature | TUI | Web | Gap | Backend work | Frontend work |
|---------|-----|-----|-----|-------------|---------------|
| List sessions | `home view` | Done | -- | -- | -- |
| Session status | status dots | Done | -- | -- | -- |
| View terminal | `Enter` | Done (PTY relay) | -- | -- | -- |
| Send keystrokes | typing in terminal | Done (PTY relay) | -- | -- | -- |
| Stop session | `x` | Done | -- | -- | -- |
| Restart session | CLI only | Done | -- | -- | -- |
| **Create session** | `n` | Missing | **Large** | `POST /api/sessions` + `POST /api/agents` (list available) | New session form (slide-over panel) |
| **Delete session** | `d` | Missing | **Medium** | `DELETE /api/sessions/{id}` with options body | Confirmation dialog with checkboxes |
| **Rename session** | `r` | Missing | **Small** | `PATCH /api/sessions/{id}` | Inline edit or rename dialog |
| **Send message** | `m` | Covered | -- | Already works via PTY terminal | -- |

### Session Display

| Feature | TUI | Web | Gap | Backend work | Frontend work |
|---------|-----|-----|-----|-------------|---------------|
| Agent view (metadata) | default view | Partial (sidebar shows title/tool/branch) | **Small** | Expand `SessionResponse` with more fields | Richer session detail panel |
| Terminal preview | preview pane | Done (full PTY) | -- | -- | -- |
| **Diff view** | `D` | Missing | **Medium** | `GET /api/sessions/{id}/diff` | Diff viewer component (unified diff rendering) |
| Session search | `/` | Missing | **Small** | -- (client-side) | Search input in sidebar header |
| Sort sessions | `o` | Missing | **Small** | -- (client-side) | Sort dropdown in sidebar header |
| View mode toggle | `t` (agent/terminal) | N/A | -- | Web always shows full terminal, not preview | -- |

### Group Management

| Feature | TUI | Web | Gap | Backend work | Frontend work |
|---------|-----|-----|-----|-------------|---------------|
| **Group hierarchy in sidebar** | tree view | Missing | **Medium** | `GET /api/groups` (returns tree) | Collapsible group tree in sidebar |
| **Create group** | implicit via session | Missing | **Small** | Part of session creation | Group field in create form |
| **Delete group** | `d` on group | Missing | **Medium** | `DELETE /api/groups/{path}` with options | Dialog with session handling options |
| **Rename group** | `r` on group | Missing | **Small** | `PATCH /api/groups/{path}` | Inline rename |
| **Move session between groups** | `r` on session | Missing | **Small** | Part of rename endpoint | Drag-and-drop or select in rename |

### Profile Management

| Feature | TUI | Web | Gap | Backend work | Frontend work |
|---------|-----|-----|-----|-------------|---------------|
| **Switch profile** | `P` | Missing | **Small** | `GET /api/profiles` + filter by profile | Profile selector in header |
| View all profiles | `[all]` view | Done (default) | -- | Already loads all profiles | -- |
| **Create profile** | in picker dialog | Missing | **Small** | `POST /api/profiles` | Input in profile selector dropdown |
| **Delete profile** | in picker dialog | Missing | **Small** | `DELETE /api/profiles/{name}` | Confirm dialog |

### Settings

| Feature | TUI | Web | Gap | Backend work | Frontend work |
|---------|-----|-----|-----|-------------|---------------|
| **Settings view** | `s` | Missing | **Large** | `GET /api/settings`, `PATCH /api/settings` | Full settings page with categories |
| Theme switching | in settings | Missing | **Medium** | `GET /api/themes`, `PATCH /api/settings` | Theme picker (with live preview) |
| All 30+ settings fields | full editor | Missing | **Large** | Settings API with field types | Form components per field type |

### Diff View

| Feature | TUI | Web | Gap | Backend work | Frontend work |
|---------|-----|-----|-----|-------------|---------------|
| **View git diff** | `D` | Missing | **Medium** | `GET /api/sessions/{id}/diff` calling existing git diff code | Syntax-highlighted unified diff component |
| **File navigation** | arrow keys | Missing | **Small** | File list in diff response | File list sidebar within diff view |
| **Edit files** | `e` (opens editor) | N/A | -- | Browser can't open local editors | -- |

### Other Features

| Feature | TUI | Web | Gap | Backend work | Frontend work |
|---------|-----|-----|-----|-------------|---------------|
| Help overlay | `?` | Missing | **Small** | -- (static content) | Keyboard shortcut help dialog |
| Update checker | background | N/A | -- | Server could check, but browser can't update binary | -- |
| Sound notifications | automatic | N/A for now | -- | Could use Web Audio API eventually | -- |
| Welcome dialog | first launch | Missing | **Small** | -- | First-visit onboarding flow |
| Worktree management | CLI only | Not priority | **Large** | Multiple endpoints | Full worktree UI |

## Recommended API Additions

All endpoints reuse existing library functions. The pattern is always:

```rust
// Load instances, find by ID, call existing method, return result
pub async fn handler(State(state), Path(id)) -> impl IntoResponse {
    let instances = state.instances.read().await;
    // ... find instance, call library function, return JSON
}
```

### Phase 1: Core CRUD (closes biggest gaps)

```
POST   /api/sessions              -- create session (wraps builder::build_instance)
DELETE /api/sessions/{id}          -- delete session (wraps deletion_poller logic)
PATCH  /api/sessions/{id}          -- rename session, change group
GET    /api/sessions/{id}/diff     -- git diff for session's repo
GET    /api/agents                 -- list available agent tools
```

### Phase 2: Organization

```
GET    /api/groups                 -- group tree
DELETE /api/groups/{path}          -- delete group
PATCH  /api/groups/{path}          -- rename group
GET    /api/profiles               -- list profiles
POST   /api/profiles               -- create profile
DELETE /api/profiles/{name}         -- delete profile
```

### Phase 3: Configuration

```
GET    /api/settings               -- current settings (global + active profile)
PATCH  /api/settings               -- update settings
GET    /api/themes                 -- available themes
```

## Effort Estimates

| Phase | Features | Backend (CC) | Frontend (CC) | Total |
|-------|----------|-------------|---------------|-------|
| **Phase 1** | Session CRUD + diff + agents list | ~30 min | ~2 hours | ~2.5 hours |
| **Phase 2** | Groups + profiles | ~20 min | ~1 hour | ~1.5 hours |
| **Phase 3** | Settings + themes | ~30 min | ~2 hours | ~2.5 hours |
| **Frontend polish** | Search, sort, keyboard shortcuts, empty states | -- | ~1 hour | ~1 hour |
| **Total** | Full TUI parity | ~1.5 hours | ~6 hours | ~7.5 hours |

## What the Web Can Do Better Than the TUI

The browser gives us capabilities the terminal can't match:

1. **Drag-and-drop** session reordering and group management
2. **Multi-select** sessions for batch operations (stop all, delete multiple)
3. **Split terminal view** -- watch 2-3 agents side by side
4. **Rich diff viewer** with syntax highlighting, inline comments
5. **Session creation wizard** with filesystem browser, visual agent picker
6. **Real-time notifications** via browser Notification API
7. **Deep linking** -- share a URL to a specific session
8. **Responsive layout** -- works on phone, tablet, desktop
9. **Clipboard integration** -- copy terminal output, paste file paths
10. **Search with preview** -- fuzzy search with instant terminal preview

## Architecture Note

The web UI is an **alternative frontend** to the same Rust library. Both the TUI and web server import from `src/session/`, `src/tmux/`, `src/git/`, etc. No business logic should live in `src/server/` -- it should all live in the shared library modules and be called by both frontends.

```
src/session/  ─┬─> src/tui/     (ratatui frontend)
src/tmux/     ─┤
src/git/      ─┤
src/containers/┴─> src/server/  (axum frontend)
                    └─> web/    (React UI)
```
