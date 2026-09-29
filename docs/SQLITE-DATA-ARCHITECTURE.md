# Mayasaba SQLite Data Architecture

## Authority
SQLite is the durable source of truth. This document defines relational implementation structure; domain contracts remain authoritative for semantics.

## Database rules
- foreign keys enabled
- WAL mode permitted for local concurrency
- schema migrations are versioned and transactional
- timestamps stored as UTC
- UUIDv7 preferred for sortable identifiers
- SHA-256 for content hashes
- secrets excluded from ordinary tables

## Core table groups
### Identity/project
projects, project_paths, project_epochs, project_status

### Agents
agents, agent_sessions, agent_capabilities

### Council
council_sessions, council_rounds, council_participants, council_positions, council_questions, council_outcomes, council_barriers

### Traceability
trace_links, trace_link_versions, trace_coverage

### Communication
messages, message_attempts, message_receipts, inbox, outbox, dead_letters

### Events
events, event_cursors

### Product/design
requirements, requirement_acceptance, decisions, decision_versions, architecture_artifacts, contracts

### Tasks
tasks, task_dependencies, task_leases, handoffs, barriers

### Workspace
workspaces, workspace_checkpoints, workspace_changes

### Execution
command_executions, process_records, builds, test_runs

### Reliability
failures, diagnoses, repairs, reviews, validation_runs

### Evidence
artifacts, evidence, evidence_links

### User interaction
user_questions, user_answers

## Council persistence invariants

- CouncilSession and CouncilRound are project-scoped and durably persisted.
- participants, positions, questions, outcomes and barriers reference their owning round.
- sealed rounds are immutable except through explicit supersession records.
- CouncilService is the sole owner of council/barrier state.

## Traceability persistence invariants

- every trace link references authoritative source and target IDs;
- link types are enum validated;
- duplicate active links are prevented;
- coverage is derived from authoritative links plus validation/certification facts;
- required links cannot be silently orphaned.

## Required invariants
- every project-scoped row references a project
- message_id unique
- event_id unique
- active lease uniqueness enforced
- sequence uniqueness per session/channel
- artifact content hash uniqueness where content-addressed
- immutable event rows are never updated for semantic correction
- current-state rows are changed only through authorized domain transactions

## Transaction boundaries
A material domain command transaction may include:
- current-state mutation
- project epoch mutation where applicable
- event append
- outbox insertion

Inbox persistence is its own durable boundary before side effects.

Integration checkpoints and workspace mutations are recorded transactionally with their authoritative metadata; filesystem operations themselves are reconciled separately.

## Index families
Indexes must cover:
project_id, status, phase, epoch, session_id, task_id, lease expiry, message delivery state, retry time, event sequence, correlation_id, artifact hash, validation status and failure fingerprint, council round/phase, barrier status, trace source/target and coverage status.

## Recovery queries
The database must support deterministic scans for:
- unprocessed outbox
- unresolved inbox
- expired leases
- stale contexts
- running executions
- non-terminal tasks
- open barriers
- retry-due messages
- incomplete validations
- uncertified projects

## Migration
A migration changes schema version atomically, preserves durable history and records migration identity. Destructive schema changes require explicit compatibility/migration design.
