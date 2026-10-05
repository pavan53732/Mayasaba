# Mayasaba Internal Application Architecture

## 1. Purpose

This document defines the internal application architecture required to implement Mayasaba as a complete Windows desktop application for software engineering and other user-authorized local-file tasks.

It extends the Master Specification and MCF-v2 documents. It does not replace them.

MCF-v2 remains the only canonical communication contract between Mayasaba and agent runtimes.

## 2. Architectural invariants

1. Mayasaba is a Windows-only local control plane.
2. The supported agent set is Hermes Agent CLI, Kilo Code CLI and OpenCode CLI (DEC-029).
3. There is no fifth reasoning model inside Mayasaba.
4. No uncontrolled direct agent-to-agent communication exists.
5. MCF-v2 is the only canonical agent communication contract.
6. SQLite is the durable runtime source of truth.
7. Tauri commands/events are the canonical UI-to-controller transport.
8. Rust owns authoritative orchestration and side effects.
9. React owns presentation and local interaction state, not project truth.
10. Agent-specific transport details remain inside adapters.
11. Material state changes are event-backed, policy-checked and replayable.
12. The integration workspace is controller-controlled.
13. Completion is evidence-backed and controller-certified.
14. No subsystem may create a competing authority for another subsystem.
15. User-selected local artifacts may be worked on within task scope; public-web retrieval is read-only and limited to user-requested research.
16. External side-effect actions and general control of unrelated applications are outside product scope.

### Intake input categories

The Initial Intake Composer carries three classes of user-provided data that must not be conflated:

- **Project identity** — immutable opaque \project_id\, never derived from a path or folder name.
- **Project display name** — initialized from the selected workspace folder name, independently editable only through a later project-settings operation.
- **Workspace authorization input** — a user-selected local Windows folder. This is a candidate authorization input, not an authorization; existence, locality and policy are separate checks (DEC-048).
- **Project intent** — free-text content persisted as `ProjectBrief` version 1.

Optional attachments are context/evidence references and are never authoritative project state (DEC-049). Their presence does not modify the `ProjectBrief`, requirements, decisions or epoch.

React may own draft versions of all of these. Only Rust-owned services may persist authoritative project state, and only the owning service may decide that any of them is material.

## 3. Internal system topology

~~~text
┌─────────────────────────────────────────────────────────────┐
│                     React Control Room                      │
│ pages • panels • timeline • dialogs • forms • stores       │
└──────────────────────────────┬──────────────────────────────┘
                               │ typed Tauri commands/events
┌──────────────────────────────▼──────────────────────────────┐
│                     Tauri Bridge                            │
│ command router • event subscriptions • validation          │
└──────────────────────────────┬──────────────────────────────┘
                               │
┌──────────────────────────────▼──────────────────────────────┐
│                    Rust Application Layer                   │
│ services • queries • commands • DTO mapping • auth context │
└──────────────┬──────────────────────────────────────────────┘
               │
       ┌───────┴─────────────────────────────────────────────┐
       │                Core Domain / Orchestrator             │
       │ lifecycle • scheduling • barriers • gates • recovery │
       └───────┬───────────────┬──────────────┬───────────────┘
               │               │              │
          MCF-v2 Bus       Task/Council   Context/Policy
               │
       ┌───────┴─────────────────────────────────────────────┐
       │ Agent Gateway / Workspace / Execution / Validation  │
       └───────────────────────┬──────────────────────────────┘
                               │
                           SQLite + Artifacts
~~~

## 4. Rust application service boundaries

Application services translate UI intent into domain commands and domain results into UI events.

Canonical services:

- ProjectService
- LifecycleService
- AgentService
- CouncilService
- RequirementService
- DecisionService
- ArchitectureService
- ContextService
- TaskService
- WorkspaceService
- ExecutionService
- BuildService
- TestService
- ValidationService
- RepairService
- EvidenceService
- ReviewService
- PolicyService
- RecoveryService
- ConfigurationService
- SimulationService
- DiagnosticsService

Rules:

- services do not bypass the domain transition guards
- services do not directly mutate unrelated subsystem state
- services emit domain events through the canonical event pipeline
- long-running work is asynchronous and cancellable
- UI request IDs map to domain correlation IDs

## 5. Domain model

Core aggregates:

### Project
Owns workspace identity, user-selected local path, task context, lifecycle status, current epoch and high-level status.

### AgentSession
Owns runtime identity, adapter state, capabilities, process state and session lifecycle.

### CouncilRound
Owns participant set, round phase, barriers, positions, questions and sealed outcome.

### Requirement
Owns requirement version, acceptance criteria, status and traceability.

### Decision
Owns decision version, authority, lock status, alternatives, rationale and supersession.

### ArchitectureArtifact
Owns structured architecture artifacts and version relationships.

### ContextSnapshot
Owns immutable context packs, epoch, digest and affected scope.

### Task
Owns objective, dependencies, ownership, status, lease, allowed paths and task-appropriate validation criteria. Objectives may target source code or other local artifacts such as documents, reports and datasets.

### Workspace
Owns the user-selected local root, allowed paths, project/worktree identity and isolation state. Whole-PC scanning is not the default.

### Execution
Owns local command/process lifecycle and evidence references.

### ValidationRun
Owns deterministic check results and verdict.

### Failure
Owns fingerprint, diagnosis links and repair history.

### Repair
Owns repair scope, attempt history and regression outcome.

### Evidence/Artifact
Owns immutable provenance, content hash and references.

### Review
Owns reviewer identity, scope, findings, severity and verdict.

Aggregates enforce invariants within their ownership boundaries.

## 6. Command/query separation

Commands:

- create_project
- open_project
- pause_project
- resume_project
- stop_project
- answer_user_question
- reopen_decision
- approve_action
- retry_task
- reassign_task
- retry_repair
- launch_agent
- stop_agent
- request_sync
- request_preview
- replay_event
- replay_dead_letter

Queries:

- get_project
- list_projects
- get_project_status
- get_agent_status
- get_council_round
- list_requirements
- list_decisions
- get_architecture
- get_context_status
- get_task_graph
- get_workspace_status
- validate_workspace
- get_build_status
- get_test_runs
- get_failures
- get_repairs
- get_reviews
- get_evidence
- get_logs
- get_communication_health

Commands can mutate state; queries cannot.

## 7. Tauri bridge contract

### Command direction

React → Tauri command → Rust application service → domain.

Each command includes:

- request_id
- project_id when project-scoped
- caller/UI session
- command payload
- client timestamp

Rust response contains:

- request_id
- status
- result or structured error
- causal event reference when applicable

### Event direction

Rust → Tauri event → React subscription/store.

Each event includes:

- event_id
- project_id
- event_type
- timestamp
- project_epoch when project-scoped
- correlation_id
- payload reference/version

### Bridge rules

- commands are typed
- events are schema-versioned
- errors are machine-readable
- no raw internal database object is exposed directly
- large logs/artifacts are referenced, not embedded
- subscription cleanup is deterministic
- canceled UI requests propagate cancellation where supported

## 8. Frontend architecture

~~~text
apps/desktop/
  src/
    app/
    routes/
    pages/
    components/
    features/
      projects/
      chat/
      council/
      requirements/
      architecture/
      decisions/
      tasks/
      agents/
      files/
      build/
      run/
      tests/
      repairs/
      evidence/
      settings/
    state/
    api/
    events/
    hooks/
    utils/
    types/
~~~

Frontend state classes:

### Server/authoritative state
Fetched from Rust services and refreshed from events.

### Event stream state
Recent immutable events rendered into timeline/notifications.

### View state
Tabs, filters, expanded panels, selected artifact, dialog visibility.

### Draft state
Unsubmitted user input only.

React must not independently invent lifecycle state.

### Chat and intake composers

Two distinct composer surfaces exist and must not be conflated:

- **Initial Intake Composer** — shown when creating a new project. The user states the idea/concept and selects the local workspace. On submission the text creates `ProjectBrief` v1. It is project truth from that point on, not a chat message.
- **Ongoing Chat Composer** — available after project creation. Free-text input creates a `UserContribution` with an advisory classification; the owning service decides whether authoritative state changes. It is never a second project-creation path.

Ownership is split so the transcript is never treated as project truth:

| Concern | Owner |
|---|---|
| User input interface / draft | Chat composer (React, draft state only) |
| Project creation | ProjectService |
| Durable representation of user intent | ProjectBrief |
| Structured requirements derived from the brief | RequirementService |
| Analysis context snapshot | ContextService |
| Deliberation over the frozen context | CouncilService |

The composer writes no requirement, decision or epoch directly. See `MEMORY-CONTEXT.md` and DEC-030/DEC-031.

## 9. Realtime event consumption

The UI maintains a project event cursor.

~~~text
subscribe(project)
→ receive events
→ verify project + schema
→ update local derived stores
→ advance cursor
→ recover missing range if sequence gap
~~~

After UI restart, the UI reloads authoritative snapshots and resumes event subscription from the current cursor.

## 10. Control Room UX state model

Major states:

- LOADING
- READY
- RUNNING
- WAITING_FOR_AGENT
- WAITING_FOR_USER
- BLOCKED
- PAUSED
- STOPPING
- RECOVERING
- FAILED
- COMPLETE

These are derived display states, not an additional project authority.

## 11. Orchestration engine

The orchestrator is a deterministic state-transition engine.

Responsibilities:

- lifecycle phase transitions
- admission control
- council barriers
- task scheduling
- lease validation
- context freshness gates
- workspace gates
- policy gates
- execution coordination
- validation gates
- repair loops
- recovery
- certification

Orchestrator cycle:

~~~text
LOAD AUTHORITATIVE STATE
→ EVALUATE READY CONDITIONS
→ SELECT NEXT ACTIONS
→ CREATE CORRELATION / CAUSATION
→ TRANSACTIONAL STATE CHANGE
→ EMIT OUTBOX EVENTS
→ DISPATCH
→ OBSERVE RESULTS
→ APPLY VALIDATION
→ ADVANCE / BLOCK / REPAIR
~~~

No agent response directly changes project phase.

## 12. Scheduler design

The scheduler maintains:

- ready queue
- blocked queue
- lease table
- capability index
- workspace-conflict index
- dependency graph
- retry budgets
- priority queues

Scheduling checks:

1. dependencies satisfied
2. project not paused/stopped
3. applicable context is current
4. agent capability available
5. workspace scope available
6. policy permits work
7. no conflicting lease/resource
8. retry budget remains

Parallel execution is allowed only for independent safe tasks.

## 13. Configuration architecture

Configuration layers:

1. application defaults
2. machine-local user configuration
3. project configuration
4. session/runtime state

Precedence:

project configuration > user configuration > application defaults

Runtime secrets are referenced, not copied into normal configuration payloads.

Configuration groups:

- UI
- agent discovery
- agent runtime limits
- communication
- retry/timeout
- council (round cap and convergence policy)
- workspace
- execution
- policy
- validation
- evidence retention
- logging/telemetry
- simulation

All configuration is versioned and schema-validated.

## 14. Agent capability degradation

Capabilities are dynamic.

When a required capability is unavailable:

~~~text
TASK READY
→ CAPABILITY CHECK
→ AVAILABLE
    → lease and run
→ UNAVAILABLE
    → alternative supported agent
    → task blocked
    → user escalation
~~~

Mayasaba must never silently substitute an unsupported mechanism.

Capability loss during execution invalidates continued actions requiring that capability and triggers recovery.

## 15. Recovery architecture

Recovery domains:

- UI recovery
- controller recovery
- bus recovery
- adapter recovery
- process recovery
- workspace recovery
- task lease recovery
- context recovery

Recovery sequence:

~~~text
DETECT FAILURE
→ RECORD FAILURE
→ QUIESCE AFFECTED SCOPE
→ VERIFY REAL PROCESS STATE
→ VERIFY WORKSPACE
→ RECONCILE BUS
→ RECONCILE LEASES
→ REBUILD CURRENT CONTEXT
→ REASSESS TASK
→ RESUME / REASSIGN / BLOCK
~~~

Historical events are preserved.

## 16. Checkpoint architecture

Checkpoint classes:

- pre-risky-operation
- pre-integration
- post-successful-validation
- recovery checkpoint

Checkpoint records identify:

- project
- workspace
- repository/head identity where applicable
- task
- agent/session
- epoch/context
- timestamp
- artifact/diff references

Rollback restores a known safe workspace state without erasing event history.

## 17. Simulation architecture

Simulation is a first-class internal test subsystem.

Simulated adapters implement the same adapter contract as real agents.

Scenario controls:

- latency
- message reordering
- duplicates
- dropped messages
- ACK loss
- malformed payloads
- stale contexts
- capability loss
- CLI crash
- timeout
- conflicting proposals
- tool failure
- build failure
- test failure
- repair regression

Simulation can run deterministic scripted scenarios and random fault-injection suites.

No simulated result may be treated as real-project evidence.

## 18. Diagnostics and observability

Structured telemetry:

- event counters
- message latency
- queue depth
- ACK latency
- retry count
- dead-letter count
- agent health
- task wait time
- lease age
- build duration
- test duration
- repair attempts
- failure fingerprints
- resource/process status

Correlation IDs allow complete operation tracing.

Logs are structured and project/session scoped.

Secrets are redacted before logging.

## 19. Subsystem interface rules

Every subsystem exposes:

- typed commands
- typed queries
- typed events
- explicit error codes
- cancellation semantics
- ownership boundary
- persistence boundary
- test contract

Subsystems communicate through:

- domain service calls for synchronous internal queries/commands
- event publication for observable state changes
- MCF-v2 for agent-facing communication
- artifact/evidence references for large data

Do not use ad-hoc callbacks between subsystems.

## 20. Canonical interface map

Core → Bus:
publish, schedule, acknowledge, replay, dead-letter, queue-health

Core → Agents:
launch, pause, resume, stop, send MCF-v2 message, health, capability

Core → Council:
open-round, submit-result, close-round, barrier-state, decision-candidate

Core → Context:
create-snapshot, compare-digest, invalidate, sync, validate-freshness

Core → Tasks:
create-task, dependency-state, lease, renew, release, reassign

Core → Workspace:
prepare-workspace, checkpoint, diff, integrate, rollback, verify

Core → Execution:
request-command, cancel-command, process-status, capture-output

Core → Validation:
request-validation, get-result, rerun

Core → Repair:
create-repair, assign, record-result, evaluate-loop

Core → Evidence:
publish-artifact, publish-evidence, resolve-reference

Core → Policy:
authorize-action, classify-command, approval-state

Core → Storage:
transaction, snapshot, event append, query, recovery scan

## 21. Dependency direction

Allowed dependency direction:

UI
→ Tauri Bridge
→ Application Services
→ Domain/Orchestrator
→ Subsystems
→ Storage/OS

Agent adapters may depend on protocol and local execution primitives but must not depend on UI.

Storage must not depend upward on orchestration or UI.

Protocol must not depend on agent-specific implementation.

This prevents architectural cycles.

## 22. Error handling

All internal errors map to stable machine-readable codes.

Error classes:

- validation
- authorization
- stale state
- capability
- workspace
- lease
- transport
- process
- persistence
- configuration
- recovery
- internal

Errors have:

- code
- severity
- retryability
- affected scope
- correlation/event references
- remediation metadata where applicable

## 23. Security boundary

Tauri is a privileged bridge.

Rust validates every command before performing privileged work.

The UI cannot directly execute PowerShell, spawn arbitrary processes, write arbitrary files, or access unrestricted filesystem paths.

All such actions pass through policy + execution/workspace services. File reads/writes are limited to the user-selected workspace and task allowed paths; out-of-scope access requires explicit approval. Public-web retrieval is read-only and limited to user-requested research. No service or adapter may expose messaging, posting to external services, form submission to public/external services, purchasing, account changes, or general control of unrelated applications.

## 24. Software build/run/test lifecycle inside Mayasaba

The system separates:

DISCOVERY → BUILD_PLAN → BUILD → LAUNCH → RUNTIME_VERIFY → TEST_PLAN → TEST → E2E → REVIEW → CERTIFICATION

A successful process launch does not equal runtime correctness. This lifecycle is for software tasks; non-software artifact tasks use only applicable acceptance and integrity checks.

## 25. Final certification path

~~~text
Acceptance-Criteria Coverage
+ Decision/Contract Consistency where applicable
+ Workspace Integrity
+ Task-Appropriate Validation PASS
+ Review PASS
+ Repair/Regression PASS where applicable
+ Evidence Complete
→ MAYASABA CERTIFICATION
→ PACKAGE where required
~~~

For software-engineering tasks, task-appropriate validation includes the applicable build, runtime, test, E2E and packaging gates defined by the software workflow. For document, research and data tasks, it includes relevant checks such as citation/format validation, diff review, record counts, invariants and recoverability. Missing required tools or evidence must yield BLOCKED/UNVERIFIED, not a passing certification.

## 26. Implementation guardrails

Before implementation, code must conform to:

- this internal architecture
- MCF-v2 protocol documents
- machine-readable schemas
- decision register
- requirements
- ownership map

If implementation needs behavior not specified by these documents, it is an architecture gap and must be explicitly resolved rather than invented silently.


## 27. Command/query/event ownership map

The machine-readable bridge contract is `schemas/tauri-bridge-v1/bridge.schema.json`. It is the single source for command, query and UI event identifiers.

Admission is not a bridge command. `prepare_workspace`, `checkpoint_workspace` and `rollback_workspace` are owned by WorkspaceService, which persists the corresponding admission record as part of the operation rather than exposing the gate as a separately invocable command. The record is surfaced to the UI through `ADMISSION_RECORDED`; the frontend never decides a gate and never submits a verdict.

### Command ownership

| Command | Owner service |
|---|---|
| create_project | ProjectService |
| open_project | ProjectService |
| pause_project | LifecycleService |
| resume_project | LifecycleService |
| stop_project | LifecycleService |
| answer_user_question | CouncilService |
| reopen_decision | DecisionService |
| approve_action | PolicyService |
| retry_task | TaskService |
| reassign_task | TaskService |
| retry_repair | RepairService |
| launch_agent | AgentService |
| stop_agent | AgentService |
| request_sync | ContextService |
| request_preview | WorkspaceService |
| replay_event | RecoveryService |
| replay_dead_letter | RecoveryService |

### Query ownership

| Query | Owner service |
|---|---|
| get_project | ProjectService |
| list_projects | ProjectService |
| get_project_status | LifecycleService |
| get_agent_status | AgentService |
| get_council_round | CouncilService |
| list_requirements | RequirementService |
| list_decisions | DecisionService |
| get_architecture | ArchitectureService |
| get_context_status | ContextService |
| get_task_graph | TaskService |
| get_workspace_status | WorkspaceService |
| validate_workspace | WorkspaceService |
| get_build_status | BuildService |
| get_test_runs | TestService |
| get_failures | RepairService |
| get_repairs | RepairService |
| get_reviews | ReviewService |
| get_evidence | EvidenceService |
| get_logs | DiagnosticsService |
| get_communication_health | DiagnosticsService |

## 28. UI event catalog

Canonical Tauri channels: `project`, `agent`, `council`, `requirement`, `decision`, `architecture`, `context`, `task`, `lease`, `workspace`, `execution`, `build`, `test`, `validation`, `repair`, `evidence`, `review`, `communication`, `diagnostics`.

Canonical event types: `PROJECT_UPDATED`, `PROJECT_PHASE_CHANGED`, `PROJECT_STATUS_CHANGED`, `AGENT_SESSION_CHANGED`, `AGENT_HEALTH_CHANGED`, `AGENT_CAPABILITY_CHANGED`, `COUNCIL_ROUND_CHANGED`, `COUNCIL_MESSAGE_RECEIVED`, `COUNCIL_BARRIER_CHANGED`, `REQUIREMENT_CHANGED`, `DECISION_CHANGED`, `ARCHITECTURE_CHANGED`, `CONTEXT_CHANGED`, `TASK_CHANGED`, `LEASE_CHANGED`, `HANDOFF_CHANGED`, `WORKSPACE_CHANGED`, `EXECUTION_CHANGED`, `BUILD_CHANGED`, `TEST_CHANGED`, `VALIDATION_CHANGED`, `FAILURE_RECORDED`, `REPAIR_CHANGED`, `ARTIFACT_PUBLISHED`, `EVIDENCE_PUBLISHED`, `REVIEW_RECORDED`, `COMMUNICATION_CHANGED`, `DEAD_LETTER_RECORDED`, `RECOVERY_STARTED`, `RECOVERY_COMPLETED`.

## 29. Bridge code generation

The source of truth is `schemas/tauri-bridge-v1/bridge.schema.json` → generated Rust command/query/event metadata + generated TypeScript bridge types. Generated files carry a generated marker and must not be hand-edited. `tools/codegen/generate-bridge.mjs` emits both surfaces — `apps/desktop/src/generated/bridge.ts` and `apps/desktop/src-tauri/src/generated/bridge.rs` — from the command, query and event enums, and its `--check` mode compares both against what the contract implies (line-ending agnostic), so the regeneration-and-diff guarantee is asserted and the "not hand-edited" rule is mechanically enforced.

The local contract gate (`npm run verify:contracts`) runs that check and additionally reads both sides of the bridge: it parses the `#[tauri::command]` functions and the `generate_handler![...]` list out of `apps/desktop/src-tauri/src/main.rs`, and the `transport(...)` call sites out of the non-generated, non-test frontend. A handler or a call naming an operation the contract does not declare fails, as does a call to a declared operation that no handler registers; a declared operation that nothing implements is reported with its count rather than failed, because the contract deliberately leads implementation. A registration list the gate cannot parse fails rather than being skipped.

The generated Rust surface is not compiled: no `mod` declaration in the shell references `apps/desktop/src-tauri/src/generated/bridge.rs`, so the build never reads it. The gate reports that fact rather than implying the compiler enforces it.

## 30. Crate/service reconciliation

The target 12-crate dependency graph is defined by `WORKSPACE-MANIFEST.md`. Application services are façades over crate/domain ownership rather than one-crate-per-service. In particular, RepairService is owned by `crates/core`; ContextService is also owned by `crates/core`. There is no `crates/repair` or `crates/context`.

## Machine-readable interface sources

Implementation source-of-truth artifacts are:

- MCF-v2: `schemas/mcf-v2/`
- service contracts: `schemas/service-contracts-v1/registry.json`
- Tauri identifiers: `schemas/tauri-bridge-v1/bridge.schema.json`
- Tauri operation metadata: `schemas/tauri-bridge-v1/payloads.json`
- adapter types/probes: `schemas/agent-adapter-v1/`
- context digest: `schemas/context-v1/context-digest.schema.json`
- SQLite schema: `schemas/sqlite-v1/schema.sql` (engine, id, timestamp and hash conventions are fixed by `schemas/sqlite-v1/manifest.json`, which declares `schemas/sqlite-v1/schema.sql` its authority)
- doctor: `schemas/doctor-v1/doctor-report.schema.json`
- recovery: `schemas/recovery-v1/recovery.schema.json`
- error codes: `schemas/error-v1/registry.json` (the canonical MCF/Tauri error-code vocabulary every subsystem's explicit error codes are drawn from, validated against `schemas/error-v1/registry.schema.json`)
- configuration: `schemas/config-v1/configuration.schema.json` (validated against the layer/group vocabulary in `schemas/config-v1/configuration-registry.json`)
- simulation: `schemas/simulation-v1/simulation.schema.json` (scenario IDs and the required fault-class vocabulary are enumerated in `schemas/simulation-v1/scenario-registry.json`)

No service or UI implementation may define a competing machine-readable contract outside these owners.


## 21. Long-running reliability composition

The runtime composes existing services rather than introducing a second hierarchy of authorities:

```text
PROJECT
  ├─ lifecycle/orchestration state
  ├─ durable event history
  └─ current requirement/decision/context epoch
       │
       ├─ TASK → TASK ATTEMPT → LEASE/FENCE
       ├─ AGENT SESSION → PROCESS TREE
       ├─ WORKSPACE → WORKSPACE REVISION/CHECKPOINT
       ├─ RESOURCE RESERVATION
       ├─ EXECUTION → ENVIRONMENT SNAPSHOT
       └─ VALIDATION → CERTIFICATION BINDING
```

`TaskAttempt` is the retry/recovery identity. `lease_version` is its fencing token. `ResourceReservation` prevents restart-time double allocation. `WorkspaceRevision` makes actual filesystem/Git state observable. `EnvironmentSnapshot` and `CertificationBinding` make validation reproducible.

`Mission`, `Worker`, `ExecutionCell`, `SwarmCell` and `Supervisor` remain composition/view concepts unless a future requirement establishes independent durable state that cannot be represented by the existing owned entities.

### Reconciliation loops
The controller periodically reconciles desired state with observed state for buses, leases, agent processes, workspaces, resources, executions and validations. Every reconciliation outcome is deterministic and auditable.

### Scheduler fairness
The scheduler orders ready work deterministically using configured priority plus fairness/aging among otherwise eligible work. Admission never violates capability, policy, lease, workspace or resource constraints to satisfy fairness.

### Liveness invariant
For a stable epoch and available required resources, an execution cannot remain silently `RUNNING` forever. Non-progress must transition to progress evidence, checkpoint/recovery, terminal state, capacity/human blocker or explicit failure.
