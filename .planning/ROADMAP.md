# Agent of Empires - Project Roadmap

## Overview

A Rust CLI/TUI application for managing tmux sessions with Docker sandboxing, supporting multiple configuration profiles.

## Phase Progress

| Phase | Name | Status | Plans |
|-------|------|--------|-------|
| 1 | Config Data Model & Shared Resolution | Complete | 1/1 |
| 2 | Session Launch & TUI Settings | In Progress | 1/2 |
| 3 | Documentation | Pending | 1/1 |

---

## Phase 1: Config Data Model & Shared Resolution ✓

**Goal:** Implement profile environment variable configuration and shared resolution utilities.

**Status:** Complete

**Plans:**
- [x] 01-01-PLAN.md — Profile environment fields in config data model

**Completion:** 2026-02-07

---

## Phase 2: Session Launch & TUI Settings

**Goal:** Integrate profile environment variables into session launch and Docker container creation.

**Status:** In Progress (1/2 complete)

**Plans:**
- [x] 02-01-PLAN.md — Add env var support to tmux session creation
- [x] 02-02-PLAN.md — Merge profile env vars with Docker container environment

**Remaining Work:**
- Task 3 from 02-02: Update container creation to use merged env vars (SBOX-01, LAUNCH-01)

**Last Activity:** 2026-02-08

---

## Phase 3: Documentation

**Goal:** Update documentation with profile environment variable features and create usage guides.

**Status:** Pending

**Plans:** 1 plan

- [ ] 03-01-PLAN.md — Document profile environment variables in configuration.md

**Deliverables:**
- Update documentation site with profile environment variables
- Create guide page for profile environment variables with use cases
- Document profile precedence rules
- Examples for common use cases (e.g., API keys per project, different tool versions)

---

## Future Phases

Phase 3+ to be defined.
