# Mayasaba Master Specification

## 1. Product definition

Mayasaba is a Windows-only, local-first workspace control plane for user-authorized work on files on the user's PC. A user selects a local workspace and asks for work on its files. Supported work includes software engineering and other local artifact tasks such as documents, research reports, data cleanup, and code refactors. Mayasaba coordinates three coding CLIs—Hermes Agent CLI, Kilo Code CLI, and OpenCode CLI—through deterministic task, workspace, policy, execution, validation, repair and evidence workflows.

For a user-requested research task, Mayasaba may read public web sources and save the resulting report and citations locally. This is read-only retrieval, not permission to send messages, publish, submit forms, purchase, change accounts, or control unrelated applications.

Mayasaba is not a fifth AI brain. The three agents retain their own intelligence, model/provider choice, tools, authentication, sessions and reasoning. They are architecturally separate sessions, not three independent implementations: Kilo Code CLI is a fork of OpenCode CLI, so Kilo and OpenCode must not be treated as independent corroboration of each other. Model/provider diversity across the three remains real.

## 2. Hard product boundaries

- Windows only.
- All Mayasaba-controlled execution occurs on the user's Windows PC.
- No cloud VM, hosted workspace or remote executor.
- No mandatory Mayasaba login/account.
- A user-selected local workspace path and task-scoped allowed paths define the filesystem boundary; Mayasaba does not scan the whole PC by default.
- User-requested public-web research may read public sources; this is read-only retrieval, not permission for external side effects.
- Mayasaba does not send email/messages, publish posts, submit forms, make purchases, change accounts, or generally control unrelated desktop applications.
- The supported agent set is exactly Hermes Agent CLI, Kilo Code CLI and OpenCode CLI (DEC-029).
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

## 4. Canonical software-engineering lifecycle

~~~text
PROJECT_CREATED → DISCOVERY → INDEPENDENT_ANALYSIS → PROPOSALS → CROSS_CRITIQUE → REBUTTAL_AND_REVISION → DISAGREEMENT_RESOLUTION → USER_INTERVIEW → PRODUCT_AND_UX_DESIGN → TECH_STACK_DEBATE → ARCHITECTURE_REVIEW → ARCHITECTURE_LOCKED → TASK_PLANNING → IMPLEMENTATION → INTEGRATION → BUILD → TEST → E2E → CROSS_AGENT_REVIEW → REPAIR (when needed) → FINAL_VALIDATION → PACKAGE → COMPLETE
~~~

Global states include PAUSED, STOPPED, BLOCKED and RECOVERING. This full lifecycle applies to software-engineering projects. Other local artifact tasks use the same scope, ownership, policy, validation and evidence principles, but execute only the stages applicable to their acceptance criteria; they do not require irrelevant build, E2E or packaging gates.

## 6. Core work loops

Mayasaba must perform the requested work, not merely generate advice. Every task uses an explicit objective, authorized workspace/paths, acceptance criteria, validation plan and evidence.

### Software-engineering tasks

idea → requirements → architecture → task DAG → isolated implementation → integration → build → run → tests → E2E/UI validation where applicable → review → diagnosis → repair → regression → certification → package where required.

### Local artifact tasks

request → workspace/file scope → plan → local document/report/data/code changes → task-appropriate integrity and acceptance checks → review → evidence-backed handoff.

For research reports, public-source retrieval is read-only; citations and source metadata are recorded locally. For data cleanup, proposed destructive changes are previewed and recoverable where feasible.

No task type is excluded solely by its app or file category. Completion is limited by available local tools and evidence; unavailable validation is reported as blocked or unverified, never silently treated as passing. An agent's “done” message never closes a task.

## 7. Authoritative truth

1. Current user-approved requirements
2. HARD_LOCK decisions
3. Versioned architecture/contracts
4. Verified workspace/repository facts
5. Objective task-appropriate evidence (build/test/E2E for software where applicable; citations and integrity checks for other artifacts)
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

A workspace is a user-selected local folder and task-scoped set of allowed paths. It may contain source code, documents, reports, datasets or other user-selected files; work is not restricted to Git repositories or a fixed application category. Mayasaba does not scan the entire PC by default. Out-of-scope file access or writes require explicit scope approval.

Git software projects use isolated worktrees/branches when possible. The integration workspace is Mayasaba-controlled. Non-Git and file-focused workspaces use scoped filesystem isolation and checkpoints. Destructive or bulk data changes require a preview and a recoverable checkpoint where feasible.

Commands are observable and recorded with executable, arguments, working directory, timing, exit result, stdout/stderr references and relevant task/session IDs. Local build, test, formatting and data-processing tools are policy-mediated. Project-local installs may proceed within the authorized workspace policy; global/admin installs require approval.

## 10. Completion contract

Completion requires the user's acceptance criteria and all validation gates applicable to the task, with evidence and controller-owned certification. Software-engineering tasks retain the applicable build, runtime, test, E2E/UI, review, regression and packaging gates. Document, research and data tasks use relevant checks such as source citations, format/schema validation, diff review, record counts, invariants and recoverability.

A task is BLOCKED or UNVERIFIED when a required local toolchain or runtime is unavailable; Mayasaba must not claim success based only on an agent report. Irrelevant software-only gates are not imposed on non-software artifact tasks.

## 11. Non-goals

Mayasaba is not:

- a cloud development environment
- a replacement model provider
- a universal agent marketplace in initial scope
- a fifth reasoning model
- a UI-only chat application
- a prompt-only “autonomous” wrapper
- an agent-controlled final authority
- a general-purpose desktop/app automation agent
- an external side-effect agent that sends messages, publishes content, submits forms, purchases items, changes accounts or controls unrelated applications

## 12. Internal application contract

The internal application architecture is defined in docs/INTERNAL-APPLICATION-ARCHITECTURE.md. It specifies the frontend, Tauri bridge, Rust services, domain aggregates, event model, orchestration engine, configuration, recovery, simulation, subsystem interfaces and dependency direction.

## 13. Source-of-truth rule

This document integrates the product contract. Detailed definitions remain in their canonical subsystem documents and machine-readable schemas. Those documents are referenced rather than redefined.
