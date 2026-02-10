# Agent of Empires - Project Roadmap

## Overview

A Rust CLI/TUI application for managing tmux sessions with Docker sandboxing, supporting multiple configuration profiles.

## Phase Progress

| Phase | Name | Status | Plans |
|-------|------|--------|-------|
| 1 | Config Data Model & Shared Resolution | Complete | 1/1 |
| 2 | Session Launch & TUI Settings | Complete | 3/3 |
| 3 | Documentation | Complete | 1/1 |
| 4 | Testing | In Progress | 1/1 |

---

## Phase 1: Config Data Model & Shared Resolution ✓

**Goal:** Implement profile environment variable configuration and shared resolution utilities.

**Status:** Complete

**Plans:**
- [x] 01-01-PLAN.md — Profile environment fields in config data model

**Completion:** 2026-02-07

---

## Phase 2: Session Launch & TUI Settings ✓

**Goal:** Integrate profile environment variables into session launch and Docker container creation.

**Status:** Complete

**Plans:**
- [x] 02-01-PLAN.md — Add env var support to tmux session creation
- [x] 02-02-PLAN.md — Merge profile env vars with Docker container environment
- [x] 02-03-PLAN.md — Container creation env vars gap closure

**Completion:** 2026-02-08

---

## Phase 3: Documentation ✓

**Goal:** Update documentation with profile environment variable features and create usage guides.

**Status:** Complete

**Plans:**
- [x] 03-01-PLAN.md — Document profile environment variables in configuration.md

**Deliverables:**
- Update documentation site with profile environment variables
- Create guide page for profile environment variables with use cases
- Document profile precedence rules
- Examples for common use cases (e.g., API keys per project, different tool versions)

**Completion:** 2026-02-08

---

## Phase 4: Testing ⏳

**Goal:** Fix compilation bugs and achieve 80%+ test coverage for all profile environment variable functionality.

**Status:** In Progress

**Plans:**
- [ ] 04-01-PLAN.md — Comprehensive testing and bug fixes

**Scope:**
- Fix compilation errors from Phase 2 implementation
- Unit tests for env var resolution logic
- Unit tests for collection and merge functions
- Integration tests for tmux sessions (non-sandbox mode)
- Integration tests for Docker containers (sandbox mode)
- Integration tests for full workflow
- Code quality checks (fmt, clippy, coverage)

**Started:** 2026-02-10

---

## Future Phases

After Phase 4 completion, v1.0 milestone will be ready for shipping. Additional work to be defined based on user feedback.
