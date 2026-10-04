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
- event rows carry a SHA-256 hash chain (see "Event hash chain")
- secrets excluded from ordinary tables

## Core table groups
### Identity/project
projects, project_paths, project_briefs, project_epochs, project_status

### Agents
agents, agent_sessions, agent_capabilities

### Council
council_sessions, council_rounds, council_participants, council_positions, council_questions, council_outcomes, council_mode_selections, council_round_roles, council_claim_grades, council_budget_ledger, council_decision_outcomes, council_outcome_agent_links, barriers

### Traceability
trace_links, trace_link_versions, trace_coverage

### Communication
messages, message_attempts, message_receipts, inbox, outbox, dead_letters

### Events
events, event_cursors, context_snapshots

### Product/design
requirements, requirement_acceptance, decisions, architecture_artifacts, contracts

### Tasks
tasks, task_dependencies, task_leases, handoffs, barriers

### Workspace
workspaces, workspace_checkpoints, workspace_changes, admissions

### Execution
command_executions, process_records, builds, test_runs

### Reliability
failures, diagnoses, repairs, reviews, validation_runs

### Evidence
artifacts, evidence, evidence_links

### User interaction
user_questions, user_answers, user_contributions

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

## Admission persistence invariants

- an admission record references its project, task and workspace;
- an `ADMITTED` verdict cannot coexist with a `FAIL` check;
- a `REFUSED` verdict carries at least one refusal reason;
- a re-evaluation supersedes via `supersedes_admission_id` rather than updating the prior row;
- the latest admission for a (task, kind) pair is the one that governs.

## Event hash chain

Every row in `events` carries `prev_hash` and `event_hash`, forming a per-project SHA-256 hash chain. The chain makes the existing immutability rule verifiable rather than merely asserted: a rewritten, deleted or reordered event is detectable, and verification reports the first divergent event.

The chain reuses the existing canonicalization rule from DEC-025 — SHA-256 over the RFC 8785 JCS serialization of the event's authoritative fields — so no second hashing convention is introduced.

Chain rules:

- the first event of a project chain uses a genesis `prev_hash` of 64 zero characters;
- `event_hash` is the SHA-256 of the JCS-canonicalized input `{prev_hash, event_id, project_id, session_id, event_type, sequence, correlation_id, causation_id, epoch, payload_json, created_at}`;
- `prev_hash` equals the `event_hash` of the immediately preceding event in the same project chain, ordered by `sequence`;
- events whose `project_id` is null form their own chain, keyed by `session_id`;
- verification recomputes the chain from persisted rows and fails at the first mismatch, naming the divergent `event_id` and `sequence`.

Limitation: a plain hash chain detects accidental corruption, partial edits, deletion and reordering. It does not detect a deliberate rewrite that recomputes every subsequent hash, because no key is involved. Adding keyed authentication would require a managed secret, which the security rules place outside ordinary configuration; that trade was declined for a single-user local application and is recorded in DEC-034.

## Required invariants
- every project-scoped row references a project
- message_id unique
- event_id unique
- active lease uniqueness enforced
- sequence uniqueness per session/channel
- artifact content hash uniqueness where content-addressed
- immutable event rows are never updated for semantic correction
- event_hash equals the recomputed chain hash and prev_hash equals the preceding event's event_hash
- every integrated changeset has an ADMITTED integration admission
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
