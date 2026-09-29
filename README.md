# Mayasaba

Mayasaba is a Windows-only, local-first autonomous software engineering platform that coordinates Claude Code CLI, Hermes Agent CLI, Kilo Code CLI, and Cline through a deterministic council, communication fabric, task engine, execution kernel, validation, repair, and evidence-backed delivery pipeline.

## Canonical repository

This repository is the **only canonical implementation repository for Mayasaba**:

- https://github.com/pavan53732/Mayasaba

All Mayasaba source code, architecture, specifications, protocols, schemas, tests, design decisions, and implementation history belong here.

## Product boundary

Mayasaba is a local Windows desktop control plane. It does not replace the four coding agents with a fifth AI brain.

The four initial agents are:

1. Claude Code CLI
2. Hermes Agent CLI
3. Kilo Code CLI
4. Cline

Each agent keeps its own model/provider, reasoning, tools, authentication, runtime, and native context. Mayasaba provides the shared deterministic engineering reality around them.

## Locked deployment constraints

- Windows-only
- Local-only execution on the user's Windows PC
- No cloud VM or remote development machine
- No required Mayasaba account or login
- User-selected local project path is authoritative
- Git worktrees/branches are preferred for agent isolation when available
- SQLite is the durable Mayasaba source of truth
- Tauri 2 + React 19 + TypeScript + Vite + Tailwind CSS + shadcn/ui
- Rust + Tokio for the controller/core
- MSI-only user-facing distribution
- Full working delivery is mandatory: build, run, test, E2E, review, repair, regression, validation, packaging

## Architecture

```text
USER
  ↓
MAYASABA CONTROL ROOM
  ↓
ORCHESTRATOR
  ↓
MCF-v2 COMMUNICATION FABRIC
  ↓
┌────────────┬────────────┬────────────┐
│ Claude     │ Hermes     │ Kilo       │
│ Code CLI   │ Agent CLI  │ Code       │
└────────────┴────────────┴────────────┘
                │
              Cline
                ↓
Workspace → Execute → Build → Test → E2E → Review → Repair → Certify
```

### Canonical subsystem ownership

- `crates/protocol` — MCF-v2 wire schemas and protocol contracts
- `crates/bus` — durable routing, ACK, retry, ordering, dedupe, backpressure, replay, dead letters
- `crates/agents` — Claude/Hermes/Kilo/Cline adapters and sessions
- `crates/core` — orchestration and project transition authorization
- `crates/tasks` — task lifecycle and leases
- `crates/council` — council rounds and barriers
- `crates/execution` — local command/process execution
- `crates/workspace` — project/worktree isolation
- `crates/validation` — deterministic validation gates
- `crates/evidence` — evidence and artifact integrity
- `crates/policy` — permissions and security policy
- `crates/storage` — SQLite persistence and event history

No subsystem may introduce a competing source of truth for another subsystem.

## Canonical lifecycle

```text
PROJECT_CREATED
→ DISCOVERY
→ INDEPENDENT_ANALYSIS
→ PROPOSALS
→ CROSS_CRITIQUE
→ REBUTTAL_AND_REVISION
→ DISAGREEMENT_RESOLUTION
→ USER_INTERVIEW
→ PRODUCT_AND_UX_DESIGN
→ TECH_STACK_DEBATE
→ ARCHITECTURE_REVIEW
→ ARCHITECTURE_LOCKED
→ TASK_PLANNING
→ IMPLEMENTATION
→ INTEGRATION
→ BUILD
→ TEST
→ E2E
→ CROSS_AGENT_REVIEW
→ REPAIR (when needed)
→ FINAL_VALIDATION
→ PACKAGE
→ COMPLETE
```

`COMPLETE` is controller-owned and evidence-backed. An agent cannot self-certify completion.

## MCF-v2

MCF-v2 is the canonical communication protocol between Mayasaba and all four CLI agents.

Key guarantees:

- versioned typed messages
- at-least-once delivery
- idempotency for side effects
- ACK/NACK semantics
- bounded retries and dead-letter handling
- causal correlation
- per-session/channel ordering
- project isolation
- project epoch and stale-context protection
- task leases
- proof-carrying handoffs
- council synchronization barriers
- pause/resume/cancel/stop propagation
- transactional outbox + receiver inbox/deduplication
- restart/crash recovery
- replayable event history

ACK never means success. Agent completion reports never become truth without evidence and validation.

## Source of truth hierarchy

1. User-approved current requirements
2. HARD_LOCK decisions
3. Versioned architecture/contracts
4. Verified repository/workspace facts
5. Objective build/test/E2E evidence
6. Persisted Mayasaba orchestration state
7. Agent proposals/reports

Conflicts are resolved explicitly; lower-level agent claims cannot silently override higher-level truth.

## Repository layout

```text
mayasaba/
├── apps/
│   └── desktop/
├── crates/
│   ├── core/
│   ├── protocol/
│   ├── bus/
│   ├── agents/
│   ├── council/
│   ├── execution/
│   ├── workspace/
│   ├── tasks/
│   ├── validation/
│   ├── evidence/
│   ├── policy/
│   └── storage/
├── schemas/
│   └── mcf-v2/
├── docs/
├── tests/
├── scripts/
└── installer/
```

## Documentation policy

Architecture is documentation-first and versioned.

Every meaningful architectural change must be classified as:

- ADDITIVE
- REFINEMENT
- REPLACEMENT
- DEPRECATION

Changes must identify affected subsystems, previous behavior, new behavior, compatibility impact, migration/reconciliation, and required tests.

Historical decisions remain traceable. Existing canonical concepts must be refined rather than duplicated.

## Current status

This repository starts as a clean architecture-first baseline. The next implementation milestone is the machine-readable MCF-v2 contract and its Rust/SQLite implementation, followed by agent adapters, council orchestration, task execution, validation, repair, and the Control Room UI.
