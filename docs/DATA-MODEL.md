# Mayasaba Durable Data Model

## Persistence authority

SQLite is the durable Mayasaba source of truth for orchestration state and event history.

The project-local `.mayasaba` directory is reserved for portable user-visible manifests and import/export packages. It is never an operational database and never competes with SQLite. SQLite in application data is the sole durable runtime source of truth.

Precedence is fixed: current SQLite state > validated `.mayasaba` import candidate > stale portable manifest. Import requires an explicit reconciliation transaction; a portable manifest never silently overwrites runtime state.

## Core entities

Project
ProjectPath
ProjectEpoch
Agent
AgentSession
AgentCapability
CouncilSession
CouncilRound
Message
MessageAttempt
MessageReceipt
Event
Requirement
Decision
ArchitectureArtifact
Contract
Task
TaskDependency
TaskLease
Handoff
Workspace
WorkspaceCheckpoint
CommandExecution
Build
TestRun
Failure
Diagnosis
Repair
Review
ValidationRun
Evidence
Artifact
Checkpoint
UserQuestion
UserAnswer
Barrier
ProjectStatus

## Important relationships

- project → many epochs, sessions, tasks, rounds and events
- session → many messages and delivery attempts/receipts
- task → many leases, handoffs, executions, validations and failures
- council round → barriers and messages
- evidence/artifacts → referenced by messages, tasks, commands, reviews and validations

## Council persistence

CouncilSession and CouncilRound are persisted in SQLite with participants, positions, questions, outcomes and barriers. CouncilService owns these records.

## Traceability persistence

Trace links are first-class durable relationships represented by `trace_links` and coverage projections. Traceability indexes authoritative objects and is not a second source of truth.

## Message persistence

Messages require durable IDs, project/session identity, delivery state, attempts, receipt state, sequence and idempotency information.

## Outbox

Outbound state transitions use a transactional outbox where the state mutation and outbound record commit together.

## Inbox

Receiver-side deduplication is persisted before a side-effecting message is executed.

## Integrity constraints

- project_id is mandatory for project-scoped records
- message_id is unique
- event_id is unique
- (session_id, channel, sequence) is unique
- active task ownership is unique
- content-addressed artifact hashes are indexed/unique where applicable
- foreign keys are enabled
- critical state/event writes are transactional

## Event history

Events are append-oriented and immutable. Current state is materialized/derived from authoritative transitions and persisted current-state records.

## Project epoch

Material changes to requirements, HARD_LOCK decisions, architecture, contracts, task graph semantics or policy increment the project epoch and invalidate affected contexts.

## Portable project metadata

`.mayasaba` has a fixed purpose: portable manifests/import-export only. Operational sessions, event history, messages, leases, council state and execution state remain in application-data SQLite.

## Secret storage

Secrets are never stored as ordinary message, log, evidence or artifact payloads. Use opaque secret references where necessary.


Relational table groups, migration rules, recovery scans and index families are defined in `SQLITE-DATA-ARCHITECTURE.md`.