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
Admission
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

### Project

Core identity and display metadata:

- `project_id` — immutable opaque project identity, independently generated and never derived from a path or folder name.
- `name` — human-readable display name, initialized from the canonical workspace folder's leaf name at creation and independently mutable through an explicit project-settings operation. Display metadata, not identity (DEC-050). A filesystem root has no leaf name and takes the explicit fallback label `Local Workspace`.
- `local_path` — canonical project workspace root, the initial filesystem and authorization boundary.

The `name` column is retained deliberately: removing it would couple display metadata to filesystem naming forever and eliminate the ability to rename a project without renaming its directory.

### ProjectContextAttachment — concept only, NOT YET DURABLE

A user-selected local file or directory associated with an intake request or a later project contribution, retained as supporting context and evidence.

This is recorded as a concept only. It has **no table, no column and no command**, and the gate does not expect it in `schema.sql`. It is documented here so an implementation agent does not invent a competing shape, not because it is implemented.

Intended properties, for the attachment slice to confirm rather than inherit: attachment identifier, project identifier, source path, kind (`FILE` or `DIRECTORY`), source scope, capture timestamp, provenance, optional content hash, lifecycle status.

Authority: attachments are contextual inputs, not project truth. Their presence does not modify `ProjectBrief`, requirements, decisions or epoch unless an owning service explicitly accepts a material state change caused by their contents (DEC-049). The storage model — reference the original local path, copy it, or ingest and index it — is undecided and belongs to the attachment slice.

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

The advisory classification is produced by the intake router (ConfigurationService policy), is stored on the `UserContribution` row, and is never read as authorization by the owning service. It exists only to route the contribution to the owning service and to label it in the timeline.

Traceability is directional: ProjectBrief → Requirement → requirement acceptance → Architecture. A requirement or decision derived from a brief retains the reference to the brief version it derives from.

## Workspace admission persistence

`Admission` is a durable decision record owned by WorkspaceService and persisted in `admissions`. One record is written per gate evaluation: kind `WORKSPACE_ADMISSION` before a lease is issued, kind `INTEGRATION_ADMISSION` before a changeset is integrated. Each record carries its subject identity, the epoch and context digest it was decided under, a per-check status drawn from the closed check vocabulary, and exactly one verdict — `ADMITTED`, `REFUSED` or `BLOCKED`. Records are append-oriented: a re-evaluation supersedes rather than rewrites, via `supersedes_admission_id`, so a refusal that was later overturned remains visible. Admission records are validated by `schemas/workspace-v1/admission.schema.json` and announced by `ADMISSION_RECORDED`.

## Council persistence

CouncilSession and CouncilRound are persisted in SQLite with participants, positions, questions, outcomes and barriers. CouncilService owns these records.

A round's termination result is persisted in `council_outcomes` with exactly one `outcome_type` — `CONVERGED`, `SYNTHESIZED`, `CAP_REACHED`, `ESCALATED` or `SEALED_WITH_OPEN_QUESTION`. A `SYNTHESIS` position is persisted in `council_positions` like any other position and is referenced by the outcome's `synthesis_position_id`. An escalation packet is persisted as the round outcome's structured body and referenced by the question's `escalation_ref`; escalation packets are validated by `schemas/council-v1/escalation.schema.json`.

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

Events are append-oriented and immutable. Current state is materialized/derived from authoritative transitions and persisted current-state records. Immutability is enforced verifiably: each event carries `prev_hash` and `event_hash` forming a per-project SHA-256 hash chain, so a rewritten, deleted or reordered event is detectable rather than merely prohibited by convention. The chain algorithm is owned by `SQLITE-DATA-ARCHITECTURE.md`; this document records only that the property is verifiable.

## Project epoch

Material changes to requirements, HARD_LOCK decisions, architecture, contracts, task graph semantics or policy increment the project epoch and invalidate affected contexts.

The same rule governs accepted user contributions. Only the owning authoritative service determines materiality — a contribution's advisory classification does not itself change the epoch, and a mislabeled contribution cannot cause or avoid an epoch transition. A material truth change increments the epoch; a change to an agent's applicable context that does not change project truth creates a new snapshot and digest at the current epoch; a contribution that changes nothing is retained in the timeline only.

## Portable project metadata

`.mayasaba` has a fixed purpose: portable manifests/import-export only. Operational sessions, event history, messages, leases, council state and execution state remain in application-data SQLite.

## Secret storage

Secrets are never stored as ordinary message, log, evidence or artifact payloads. Use opaque secret references where necessary.


Relational table groups, migration rules, recovery scans and index families are defined in `SQLITE-DATA-ARCHITECTURE.md`.