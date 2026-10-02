# Mayasaba Durable Data Model

## Persistence authority

SQLite is the durable Mayasaba source of truth for orchestration state and event history.

The project-local `.mayasaba` directory is reserved for portable user-visible manifests and import/export packages. It is never an operational database and never competes with SQLite. SQLite in application data is the sole durable runtime source of truth.

Precedence is fixed: current SQLite state > validated `.mayasaba` import candidate > stale portable manifest. Import requires an explicit reconciliation transaction; a portable manifest never silently overwrites runtime state.

## Core entities

Project
ProjectPath
ProjectBrief
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
UserContribution
Barrier
ProjectStatus

## Important relationships

- project → many epochs, sessions, tasks, rounds and events
- project → many ProjectBrief versions, linked by supersession
- project → many UserContributions
- session → many messages and delivery attempts/receipts
- task → many leases, handoffs, executions, validations and failures
- council round → barriers and messages
- evidence/artifacts → referenced by messages, tasks, commands, reviews and validations

## Project intent and user contribution persistence

`ProjectBrief` is the canonical, versioned representation of the user's stated project intent. It follows the existing `requirements` versioning pattern: each version is immutable and carries a `supersedes_brief_id` link to the version it replaces. The brief version current when DISCOVERY closes is the analysis anchor for that lineage; it is referenced by the resulting context snapshots and council rounds and is never rewritten by a later version.

`UserContribution` durably records a free-text user message submitted after project creation, together with its advisory classification and its outcome. It is a record of what the user contributed, not an authority for project truth: the owning domain service remains authoritative for any resulting requirement, decision, epoch or context mutation. A contribution may reference its originating UI message/event identifiers.

Traceability is directional: ProjectBrief → Requirement → requirement acceptance → Architecture. A requirement or decision derived from a brief retains the reference to the brief version it derives from.

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

The same rule governs accepted user contributions. Only the owning authoritative service determines materiality — a contribution's advisory classification does not itself change the epoch, and a mislabeled contribution cannot cause or avoid an epoch transition. A material truth change increments the epoch; a change to an agent's applicable context that does not change project truth creates a new snapshot and digest at the current epoch; a contribution that changes nothing is retained in the timeline only.

## Portable project metadata

`.mayasaba` has a fixed purpose: portable manifests/import-export only. Operational sessions, event history, messages, leases, council state and execution state remain in application-data SQLite.

## Secret storage

Secrets are never stored as ordinary message, log, evidence or artifact payloads. Use opaque secret references where necessary.


Relational table groups, migration rules, recovery scans and index families are defined in `SQLITE-DATA-ARCHITECTURE.md`.