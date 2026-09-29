# Mayasaba Master Architecture

## 1. Purpose

Mayasaba is a deterministic local control plane for autonomous software engineering using four independent coding CLIs.

## 2. Core principle

Mayasaba is not a fifth AI brain. It coordinates independent agent runtimes against one authoritative project reality.

## 3. End-to-end system

```text
User Idea
  ↓
Project + Local Path
  ↓
Preflight / Agent Discovery
  ↓
Independent Analysis
  ↓
Council / Debate
  ↓
Targeted User Interview
  ↓
Product + UX
  ↓
Stack Debate
  ↓
Architecture Review
  ↓
Architecture Lock
  ↓
Task DAG
  ↓
Isolated Implementation
  ↓
Integration
  ↓
Build
  ↓
Run
  ↓
Test
  ↓
E2E / UI
  ↓
Cross-Agent Review
  ↓
Diagnose / Repair / Retest
  ↓
Final Validation
  ↓
Package
  ↓
Full Working Project
```

## 4. Runtime architecture

### Desktop

- Tauri 2
- React 19
- TypeScript
- Vite
- Tailwind CSS
- shadcn/ui

### Controller

- Rust
- Tokio
- typed internal events
- deterministic orchestration

### Persistence

- SQLite
- append-oriented event history
- transactional state mutations
- transactional outbox
- receiver inbox/deduplication

### Agent integration

Adapters:

- ClaudeCodeAdapter
- HermesAdapter
- KiloCodeAdapter
- ClineAdapter

Native agent protocols remain adapter-internal. MCF-v2 remains the canonical interoperability contract.

## 5. Workspace isolation

Git repositories should use isolated worktrees/branches for concurrent agents.

The integration workspace remains Mayasaba-controlled.

Non-Git projects use controlled filesystem checkpoints and scoped access.

## 6. Permission model

- READ_ONLY
- SAFE_WRITE
- PROJECT_WRITE
- EXECUTE
- INSTALL
- ADMIN_REQUIRED

Default scope is the selected project root.

## 7. Evidence-backed completion

A task or project is complete only after objective evidence is captured and appropriate deterministic validation passes.

## 8. Design governance

One canonical owner per subsystem. No shadow source of truth.

See also:

- `docs/MCF-V2-PROTOCOL.md`
- `docs/MCF-V2-MACHINE-READABLE-CONTRACT.md`
- `docs/DESIGN-GOVERNANCE.md`
