# Mayasaba

Mayasaba is a Windows-only, local-first autonomous software engineering platform that coordinates Claude Code CLI, Hermes Agent CLI, Kilo Code CLI, and Cline through a deterministic council, communication fabric, task engine, execution kernel, validation, repair, and evidence-backed delivery pipeline.

## Canonical repository

**This repository is the only canonical implementation repository for Mayasaba:**

https://github.com/pavan53732/Mayasaba

All Mayasaba source code, architecture, specifications, protocol schemas, tests, design decisions, and implementation history belong here.

## What Mayasaba is

Mayasaba is a **control plane for four independent coding agents**.

It is not a fifth AI brain. Each supported CLI keeps its own model/provider, context, reasoning, tools, authentication and runtime. Mayasaba supplies the shared deterministic project state, communication, orchestration, workspace, execution, validation and evidence systems.

## Four initial agents

1. Claude Code CLI
2. Hermes Agent CLI
3. Kilo Code CLI
4. Cline

The architecture is adapter-based for future extension, but no additional agent is part of the initial supported set.

## Locked boundaries

- Windows-only.
- Local-only execution on the user's Windows PC.
- No cloud VM, remote executor or hosted development workspace.
- No required Mayasaba login/account.
- User-selected local project path is authoritative.
- Git worktrees/branches are preferred for concurrent agent isolation.
- SQLite is the durable Mayasaba source of truth.
- Tauri 2 + React 19 + TypeScript + Vite + Tailwind CSS + shadcn/ui.
- Rust + Tokio for the controller/core.
- User-facing distribution is MSI only.
- Full working software delivery is mandatory; planning-only completion is not accepted.

## Architecture at a glance

~~~text
USER
  ↓
CONTROL ROOM
  ↓
MAYASABA ORCHESTRATOR
  ↓
MCF-v2 COMMUNICATION FABRIC
  ↓
┌──────────────┬──────────────┬──────────────┬──────────────┐
│ Claude Code  │ Hermes Agent │ Kilo Code    │ Cline        │
└──────────────┴──────────────┴──────────────┴──────────────┘
                         ↓
             WORKSPACE + EXECUTION
                         ↓
        BUILD → TEST → E2E → REVIEW
                         ↓
               DIAGNOSE → REPAIR
                         ↓
                REGRESSION → CERTIFY
                         ↓
                     PACKAGE
~~~

## Canonical lifecycle

~~~text
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
→ REPAIR
→ FINAL_VALIDATION
→ PACKAGE
→ COMPLETE
~~~

REPAIR is entered only when required. COMPLETE is controller-owned and evidence-backed.

## Canonical subsystem ownership

| Responsibility | Canonical owner |
|---|---|
| Protocol schemas | crates/protocol |
| Communication bus | crates/bus |
| Agent adapters/sessions | crates/agents |
| Orchestration/state transitions | crates/core |
| Council | crates/council |
| Context synchronization | core context orchestration |
| Tasks and leases | crates/tasks |
| Workspace/worktrees | crates/workspace |
| Local execution | crates/execution |
| Validation | crates/validation |
| Evidence/artifacts | crates/evidence |
| Security/policy | crates/policy |
| SQLite persistence | crates/storage |

No subsystem may create a competing authority for another subsystem.

## MCF-v2

MCF-v2 is the canonical communication protocol connecting Mayasaba and the four CLIs.

It provides:

- typed versioned envelopes
- at-least-once delivery
- ACK/NACK
- idempotent side effects
- ordering and causality
- retries and dead letters
- priority lanes and backpressure
- project isolation
- project epochs
- ContextPacks and stale-context protection
- task leases
- proof-carrying handoffs
- council barriers
- pause/resume/cancel/stop propagation
- transactional outbox and receiver inbox/deduplication
- replay and crash recovery

**ACK is receipt, not success. An agent report is not authoritative truth.**

## Truth hierarchy

1. Current user-approved requirements
2. HARD_LOCK decisions
3. Versioned architecture/contracts
4. Verified repository/workspace facts
5. Objective build/test/E2E evidence
6. Persisted Mayasaba orchestration state
7. Agent proposals/reports

## Documentation

Start with docs/INDEX.md.

The documentation is intentionally split by ownership to avoid one oversized, conflicting specification.

## Repository layout

~~~text
mayasaba/
├── apps/desktop/
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
├── schemas/mcf-v2/
├── docs/
├── tests/
├── scripts/
└── installer/
~~~

## Documentation governance

Architecture changes must be classified as ADDITIVE, REFINEMENT, REPLACEMENT or DEPRECATION. Existing canonical terms, schemas, state owners and sources of truth must not be silently redefined.

## Current status

The repository is in a **documentation-first architecture baseline**. The next implementation gate is the actual MCF-v2 machine-readable schema package and Rust/SQLite conformance implementation, followed by the four adapters, council, tasks, execution, validation/repair and Control Room.
