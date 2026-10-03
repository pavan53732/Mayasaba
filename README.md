# Mayasaba

Mayasaba is a Windows-only, local-first workspace control plane for user-authorized work on files on the user's PC. Software engineering remains first-class, alongside document work, research reports, data cleanup, and code refactors. Mayasaba coordinates Hermes Agent CLI, Kilo Code CLI, and OpenCode CLI through a deterministic council, communication fabric, task engine, execution kernel, validation, repair, and evidence-backed delivery pipeline.

## Canonical repository

**This repository is the only canonical implementation repository for Mayasaba:**

https://github.com/pavan53732/Mayasaba

All Mayasaba source code, architecture, specifications, protocol schemas, tests, design decisions, and implementation history belong here.

## What Mayasaba is

Mayasaba is a **control plane for three CLI agents**. It can coordinate user-authorized tasks on local files across supported task types; the available agent capabilities and evidence determine what can be completed and verified.

It is not a fifth AI brain. Each supported CLI keeps its own model/provider, context, reasoning, tools, authentication and runtime. Mayasaba supplies the shared deterministic workspace state, communication, orchestration, execution, validation and evidence systems.

## Supported agents

1. Hermes Agent CLI
2. Kilo Code CLI
3. OpenCode CLI

The architecture is adapter-based for future extension, but changing this set is a governed change to DEC-029, not a documentation edit.

## Locked boundaries

- Windows-only.
- Local-only execution on the user's Windows PC.
- No cloud VM, remote executor or hosted development workspace.
- No required Mayasaba login/account.
- The user-selected local workspace and task-scoped allowed paths are authoritative; Mayasaba does not scan the whole PC by default.
- User-requested research may retrieve public web sources in read-only mode; reports and citations are saved locally.
- External side-effect actions (email/messages, posts, public/external form submissions, purchases, account changes) and general control of unrelated applications are outside product scope.
- Git worktrees/branches are preferred for concurrent agent isolation.
- SQLite is the durable Mayasaba source of truth.
- Tauri 2 + React 19 + TypeScript + Vite + Tailwind CSS + shadcn/ui.
- Control Room visual language: minimal/functional base with bento-grid layout (DEC-032).
- Rust + Tokio for the controller/core.
- User-facing distribution is MSI only.
- Completion must deliver the requested local artifacts with evidence; software tasks require a working, validated software deliverable rather than planning alone.

## Architecture at a glance

The execution/validation chain below depicts the software-engineering path; other local artifact tasks use the applicable checks described in the canonical specification.

~~~text
USER
  ↓
CONTROL ROOM
  ↓
MAYASABA ORCHESTRATOR
  ↓
MCF-v2 COMMUNICATION FABRIC
  ↓
┌──────────────┬──────────────┬──────────────┐
│ Hermes Agent │ Kilo Code    │ OpenCode     │
└──────────────┴──────────────┴──────────────┘
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

## Canonical software-engineering lifecycle

The lifecycle below is the full software-engineering workflow. Other local artifact tasks use only applicable planning, editing, review, validation and evidence stages; they do not inherit irrelevant software build/package gates.

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

MCF-v2 is the canonical communication protocol connecting Mayasaba and the three CLIs.

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
5. Objective task-appropriate evidence (build/test/E2E for software where applicable; citations and integrity checks for other artifact tasks)
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

## Local validation

Contract verification runs locally on the user's Windows PC — there is no hosted CI (DEC-036). From the repository root:

~~~text
npm run verify:contracts
~~~

This runs `tools/contracts/verify.mjs`, which checks the MCF registry and protocol/message/event enums, the Tauri bridge identifiers against their declared owners in `workspace.manifest.json`, the agent-adapter set and declared controls, the existence of the declared manifests, and that each crate's `Cargo.toml` names its declared `mayasaba-*` dependencies. A non-zero exit means contract drift; it must be resolved before handoff, not waived. It requires no network access and no CI service.

The gate is wired to a version-controlled pre-commit hook, so it is not merely available but run:

~~~text
git config core.hooksPath .githooks
~~~

`.githooks/pre-commit` lives in the repository, so there is one copy and nothing to install or keep in sync — `core.hooksPath` is a pointer, not a copy, and `git config core.hooksPath` reports the whole state of the mechanism. The gate prints whether the hook is enabled in its summary line. That report is informational and never a failure, so a contributor who deliberately opts out is not blocked.

## Current status

The repository is in a **documentation-first architecture baseline**. The next implementation gate is the actual MCF-v2 machine-readable schema package and Rust/SQLite conformance implementation, followed by the three adapters, council, tasks, execution, validation/repair and Control Room.
