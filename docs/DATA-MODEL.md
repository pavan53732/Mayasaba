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
ProjectContextAttachment
Agent
AgentSession
AgentCapability
CouncilSession
CouncilRound
CouncilModeSelection
CouncilRoundRole
CouncilClaimGrade
CouncilBudgetLedger
CouncilDecisionOutcome
CouncilOutcomeAgentLink
Message
MessageAttempt
MessageReceipt
Event
Requirement
Decision
ArchitectureArtifact
Contract
Task
TaskAttempt
TaskDependency
TaskLease
Handoff
Workspace
WorkspaceCheckpoint
WorkspaceRevision
ResourceReservation
EnvironmentSnapshot
CertificationBinding
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

### ProjectContextAttachment

A user-selected local file or directory associated with an intake request **or a later project contribution**, retained as supporting context and evidence.

Durable in `project_context_attachments`. The table stores a reference and its provenance and never a snapshot: `source_path` is immutable once written, `authorized_scope` records the workspace root the path was validated against at attach time, and `content_hash` and `context_evidence_id` are nullable because capture and consume are separate explicit operations (DEC-106). The entity and its lifecycle are owned by `AttachmentService` in `crates/core` (DEC-107).

**Storage model (DEC-106, settled).** An attachment is a **durable local reference with captured provenance**. Attaching records that the user selected this path at this time inside this authorized scope, and nothing more: no read, no copy, no hash, no index. Content ingestion and indexing are a **separate explicit operation** and do not occur merely because a file was attached. An attachment is an **optional capability**; the chat composer must support it, and an attachment is never a prerequisite for submitting a normal user message.

Three operations, which must not be collapsed: **attach** (record the reference and provenance), **capture** (compute a hash and produce an Artifact/Evidence record — explicit and on request only), and **consume** (an owning service accepts a material change caused by the contents — explicit and separate).

Intended properties, confirmed by the attachment slice and implemented as the columns of `project_context_attachments`:

- attachment identifier — `attachment_id`
- project identifier — `project_id`
- source path — **immutable once written**; a re-attachment is a new identity, never an update — `source_path`
- kind (`FILE` or `DIRECTORY`); for a directory this means the scope, not a snapshot, and enumeration is never durable — `kind`
- source scope — `authorized_scope`, the workspace root the path was validated against at attach time
- capture timestamp — `captured_at`
- provenance — `provenance`, the surface the reference was offered from
- optional content hash — **set only by an explicit capture** — `content_hash`
- lifecycle status — `SELECTED | PENDING | ACCEPTED | REJECTED` — `lifecycle_state`
- optional context/evidence links — **set only by an explicit consume** — `context_evidence_id`

`content_hash` and the links are nullable by design so that capture and consume can be added later **without replacing the attachment's identity**.

**Durability semantics (DEC-106).** A modified source does not invalidate the attachment, because content identity is not implied; a consumer needing it computes a hash and records the comparison as a check outcome. A deleted or moved source leaves the row **in place** — history is append-only and `source_path` is never rewritten — and a later read fails with a typed code that is recorded rather than rendered as an empty attachment. Resolvability is therefore a **check performed when read, not a stored state**, reusing the `Admission` pattern in `schemas/workspace-v1/admission.schema.json`. A bare path reference **cannot be cited as evidence**: evidence requires immutable provenance and a content hash (DEC-102), so citing an attachment requires a prior capture.

Authority: attachments are contextual inputs, not project truth. Their presence does not modify `ProjectBrief`, requirements, decisions or epoch unless an owning service explicitly accepts a material state change caused by their contents (DEC-049). An attachment belongs to the **project**, not to an epoch, so consuming one never advances the epoch on its own.

Ownership (DEC-107): the entity, its project association and its lifecycle are owned by `AttachmentService` in `crates/core`. Path existence, locality and authorized-scope validation remain with `WorkspaceService` in `crates/workspace` (DEC-048), and explicit capture, content hashing and evidence provenance remain with `EvidenceService` in `crates/evidence` (DEC-102). One owner for the entity, with the other concerns left where they already belonged.

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

Ownership: `ProjectService` in `crates/core` owns the record and the `record_user_contribution` command that writes it. DEC-030 makes a material contribution increment `project_epoch`, and `ProjectService` already owns `projects`, `project_briefs` and the epoch, so the record is written by the service that owns the fact the record is about. The advisory classification stays where it already belonged — the intake router, as ConfigurationService policy — and `ProjectService` stores the label without acting on it. Rows are append-only: there is no update path and no delete path, so a later ruling is a later row rather than a rewrite of this one.

The advisory classification is produced by the intake router (ConfigurationService policy), is stored on the `UserContribution` row, and is never read as authorization by the owning service. It exists only to route the contribution to the owning service and to label it in the timeline.

Three vocabularies are closed by CHECK constraints in `schemas/sqlite-v1/schema.sql`, because they were free `TEXT NOT NULL` columns declared in no schema at all, so any string could be stored and nothing could state what the members were. `classification` is `MATERIAL`, `CONTEXT` or `COMMENTARY` — DEC-030's own three outcomes. `classification_source` is `INTAKE_ROUTER`. `result_type` is `EPOCH_ADVANCED`, `CONTEXT_SNAPSHOT`, `NO_CHANGE` or `PENDING`, where `PENDING` means the contribution is recorded and no owning service has ruled on it yet. `epoch_after` is never below `epoch_before`.

What is not implemented is routing. `record_user_contribution` records the contribution with `result_type = PENDING` and an unchanged epoch pair, because no operation yet carries the text to the owning service that would decide materiality and produce a real outcome. The record therefore says exactly what happened — the user contributed this text and it was labelled for routing — and does not claim project truth changed.

Traceability is directional: ProjectBrief → Requirement → requirement acceptance → Architecture. A requirement or decision derived from a brief retains the reference to the brief version it derives from.

## Workspace admission persistence

`Admission` is a durable decision record owned by WorkspaceService and persisted in `admissions`. One record is written per gate evaluation: kind `WORKSPACE_ADMISSION` before a lease is issued, kind `INTEGRATION_ADMISSION` before a changeset is integrated. Each record carries its subject identity, the epoch and context digest it was decided under, a per-check status drawn from the closed check vocabulary, and exactly one verdict — `ADMITTED`, `REFUSED` or `BLOCKED`. Records are append-oriented: a re-evaluation supersedes rather than rewrites, via `supersedes_admission_id`, so a refusal that was later overturned remains visible. Admission records are validated by `schemas/workspace-v1/admission.schema.json` and announced by `ADMISSION_RECORDED`.

## Council persistence

CouncilSession and CouncilRound are persisted in SQLite with participants, positions, questions, outcomes and barriers. CouncilService owns these records.

A round's termination result is persisted in `council_outcomes` with exactly one `outcome_type` — `CONVERGED`, `SYNTHESIZED`, `CAP_REACHED`, `ESCALATED` or `SEALED_WITH_OPEN_QUESTION`. A `SYNTHESIS` position is persisted in `council_positions` like any other position and is referenced by the outcome's `synthesis_position_id`. An escalation packet is persisted as the round outcome's structured body and referenced by the question's `escalation_ref`; escalation packets are validated by `schemas/council-v1/escalation.schema.json`.

`CouncilModeSelection`, `CouncilRoundRole`, `CouncilClaimGrade`, `CouncilBudgetLedger`, `CouncilDecisionOutcome` and `CouncilOutcomeAgentLink` are durable decision-quality records owned by CouncilService and persisted in `council_mode_selections`, `council_round_roles`, `council_claim_grades`, `council_budget_ledger`, `council_decision_outcomes` and `council_outcome_agent_links`. A mode selection is persisted for every material decision point, including `SOLO`, which persists a record and no round; mode selections are validated by `schemas/council-v1/mode-selection.schema.json`. Grades, round roles and budget-ledger entries are controller-computed records, not agent-supplied fields. Outcome records are validated by `schemas/council-v1/decision-outcome.schema.json`.

`council_decision_outcomes` references `decisions(decision_id)`, and an outcome's supporting evidence is referenced directly by `validation_evidence_id` rather than through `evidence_links`, because `evidence_links` links evidence to artifacts only. Outcome records are append-only: a supersession appends through `supersedes_outcome_id` and there is no update or delete path. Outcome tracking is informational only and never affects routing, thresholds, mode selection or authority.

## Traceability persistence

Trace links are first-class durable relationships represented by `trace_links` and coverage projections. Traceability indexes authoritative objects and is not a second source of truth. Reliability provenance uses the closed `trace-v1` vocabulary to connect TaskAttempts to checkpoints and executions, and execution/validation records to environment snapshots.


## Message persistence

Messages require durable IDs, project/session identity, delivery state, attempts, receipt state, sequence and idempotency information.

## Outbox

Outbound state transitions use a transactional outbox where the state mutation and outbound record commit together.
`outbox.dispatch_state` is `PENDING` or `FAILED` while an entry is still claimable and `DISPATCHED` or `ABANDONED`
once it is not, so a failed handover leaves the entry claimable and a terminated message leaves it abandoned
(DEC-058).

An enqueue is idempotent on `project_id + operation_id`, and **only a live claim counts**: a message that has
terminated no longer absorbs an enqueue for the same operation, because once an operation has terminated,
re-enqueueing it is what replay is (DEC-065).

## Inbox

Receiver-side deduplication is persisted before a side-effecting message is executed. `inbox.processing_state` is
`PERSISTED`, `ACKED`, `PROCESSING`, `PROCESSED`, `RETRYING`, `EXPIRED` or `DEAD_LETTER` (DEC-059, DEC-062), and the
terminal event is recorded on the row. An arriving message is born `DISPATCHED` - its sender dispatched it, so the
receiver's part begins at `RECEIVED` - and a redelivery after a requeue is told apart from a duplicate by the
delivery state (DEC-061).

## Delivery states

`messages.delivery_state` carries **one** machine for a message in both directions, and its vocabulary is the
`message_delivery` machine's. **The four state columns - `messages.delivery_state`, `outbox.dispatch_state`,
`inbox.processing_state` and `message_receipts.receipt_state` - carry no `CHECK` constraint**, and none can be
added: the tables exist and the schema applies with `CREATE TABLE IF NOT EXISTS`, so there is no path to an
existing database. The vocabulary is enforced by the bus and by the contract gate, which reads the delivery edges
out of the Rust and holds them to the machine (DEC-066); a state name written where no edge is involved is not
checked.

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

## Reliability extensions

The response/recovery refinements are composed from existing authorities rather than introduced as competing domain objects.

- `TaskAttempt` is a durable child of `Task`. A task remains the stable unit of acceptance; each retry/reassignment creates a new attempt identity. `TaskAttempt.fence_token` is exactly the active `TaskLease.lease_version` and is never an independent authority.
- `ResourceReservation` is owned by scheduling/execution admission. It binds a task and lease to a typed local resource such as CPU, RAM, GPU, disk, port, workspace, process slot, agent slot or toolchain slot. Reservation state is durable so restart cannot double-allocate the same resource.
- `WorkspaceRevision` is the observed revision identity of a workspace. It records repository head when applicable plus content/manifest hashes and whether the observation is expected, verified, drifted or unknown.
- `EnvironmentSnapshot` records the execution/validation environment actually observed: OS identity, runtime/toolchain versions and the environment-policy hash. It is evidence provenance, not configuration authority.
- `CertificationBinding` is an immutable certification claim bound to exact artifact hashes, workspace revision, environment snapshot and validator/test-suite versions. `ASSERTED` is permitted only from a `PASS` validation. Later invalidation/expiry is appended as a new binding carrying `supersedes_binding_id`; prior rows are never rewritten.

`Mission`, `MissionJournal`, `Worker`, `ExecutionCell`, `SwarmCell` and `Supervisor` are intentionally not additional sources of truth. A project is the durable lifecycle root; events are the journal; an agent session plus process records describe a worker runtime; task/lease/workspace/context/execution rows compose an execution cell; and swarm/supervision are controller coordination views.

A safe point is represented by an existing `WorkspaceCheckpoint` with kind `SAFE_POINT`; no second checkpoint entity is introduced.


`Mission`, `MissionJournal`, `Worker`, `ExecutionCell`, `SwarmCell` and `Supervisor` are intentionally not additional sources of truth. A project is the durable lifecycle root; events are the journal; an agent session plus process records describe a worker runtime; task/lease/workspace/context/execution rows compose an execution cell; and swarm/supervision are controller coordination views.

Task-attempt relationships to existing execution/checkpoint/admission records use the existing generic `trace_links` authority rather than adding non-migratable columns to existing tables.
