# Mayasaba Master Specification

## 1. Product definition

Mayasaba is a Windows-only, local-first autonomous software-engineering control plane. A user provides a project idea and a local project path. Mayasaba coordinates four independent coding CLIs—Claude Code CLI, Hermes Agent CLI, Kilo Code CLI, and Cline—through a deterministic communication, council, task, execution, validation, repair and evidence system until a full working project is delivered.

Mayasaba is not a fifth AI brain. The four agents retain independent intelligence, model/provider choice, tools, authentication, sessions and reasoning.

## 2. Hard product boundaries

- Windows only.
- All Mayasaba-controlled execution occurs on the user's Windows PC.
- No cloud VM, hosted workspace or remote executor.
- No mandatory Mayasaba login/account.
- User-selected local path is the project boundary.
- Initial supported agents are exactly Claude Code CLI, Hermes Agent CLI, Kilo Code CLI and Cline.
- CLI-native protocols remain adapter-internal.
- SQLite is the local durable source of Mayasaba state/history.
- User-facing distribution is MSI only.

## 3. System layers

1. Control Room UI
2. Orchestrator
3. MCF-v2 Communication Fabric
4. Agent Gateway / Adapters
5. Council Engine
6. Context Synchronizer
7. Task/DAG Engine
8. Workspace Manager
9. Local Execution Kernel
10. Validation / Repair Engine
11. Evidence Engine
12. Policy Engine
13. SQLite Storage

## 4. Canonical lifecycle

~~~text
PROJECT_CREATED → DISCOVERY → INDEPENDENT_ANALYSIS → PROPOSALS → CROSS_CRITIQUE → REBUTTAL_AND_REVISION → DISAGREEMENT_RESOLUTION → USER_INTERVIEW → PRODUCT_AND_UX_DESIGN → TECH_STACK_DEBATE → ARCHITECTURE_REVIEW → ARCHITECTURE_LOCKED → TASK_PLANNING → IMPLEMENTATION → INTEGRATION → BUILD → TEST → E2E → CROSS_AGENT_REVIEW → REPAIR (when needed) → FINAL_VALIDATION → PACKAGE → COMPLETE
~~~

Global states include PAUSED, STOPPED, BLOCKED and RECOVERING.

## 5. Core engineering loop

The system must perform real work, not merely generate advice:

idea → requirements → architecture → task DAG → isolated implementation → integration → build → run → tests → E2E/UI validation → review → diagnosis → repair → regression → certification → package.

An agent's “done” message never closes this loop.

## 6. Authoritative truth

1. Current user-approved requirements
2. HARD_LOCK decisions
3. Versioned architecture/contracts
4. Verified workspace/repository facts
5. Objective build/test/E2E evidence
6. Persisted Mayasaba orchestration state
7. Agent proposals/reports

Conflicts are explicit and traceable.

## 7. Independent state machines

Mayasaba must not implement one giant state enum. Independent authoritative machines govern:

- project/orchestrator
- agent session
- message delivery
- context
- task/lease
- council round/barrier
- handoff
- execution
- validation

The orchestrator derives aggregate project state from these systems.

## 8. MCF-v2 invariants

- No unmanaged direct agent-to-agent channel.
- At-least-once delivery with idempotency.
- ACK means receipt, not success.
- Material actions require identity, session, project, workspace, policy, capability, lease, current context and relevant decision/contract state.
- Stale context cannot execute current material work.
- Duplicate messages cannot duplicate material side effects.
- Historical events are immutable and replayable.
- User interruption traffic has highest operational priority.
- COMPLETE is controller-certified and evidence-backed.

## 9. Workspace and execution

Git repositories use isolated worktrees/branches when possible. The integration workspace is Mayasaba-controlled. Non-Git projects use scoped filesystem isolation and checkpoints.

Commands are observable and recorded with executable, arguments, working directory, timing, exit result, stdout/stderr references and relevant task/session IDs.

## 10. Completion contract

COMPLETE requires:

- requirements coverage
- architecture/contract consistency
- source completeness
- successful build
- successful required runtime launch
- critical workflows functioning
- persistence functioning where applicable
- tests passing
- E2E/UI validation where applicable
- cross-agent review passing
- high-severity findings resolved
- regression passing
- final package/build artifact
- evidence bundle
- controller certification

## 11. Non-goals

Mayasaba is not:

- a cloud development environment
- a replacement model provider
- a universal agent marketplace in initial scope
- a fifth reasoning model
- a UI-only chat application
- a prompt-only “autonomous” wrapper
- an agent-controlled final authority

## 12. Internal application contract

The internal application architecture is defined in docs/INTERNAL-APPLICATION-ARCHITECTURE.md. It specifies the frontend, Tauri bridge, Rust services, domain aggregates, event model, orchestration engine, configuration, recovery, simulation, subsystem interfaces and dependency direction.

## 13. Source-of-truth rule

This document integrates the product contract. Detailed definitions remain in their canonical subsystem documents and machine-readable schemas. Those documents are referenced rather than redefined.
