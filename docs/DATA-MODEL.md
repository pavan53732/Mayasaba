# Mayasaba Durable Data Model

## Persistence authority

SQLite is the durable Mayasaba source of truth for orchestration state and event history.

The project-local .mayasaba directory may contain portable manifests for requirements, decisions, tasks and evidence. Larger operational/session data may remain in the application data directory.

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

## Secret storage

Secrets are never stored as ordinary message, log, evidence or artifact payloads. Use opaque secret references where necessary.
