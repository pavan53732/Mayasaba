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
- `sequence` is not unique or `NOT NULL` in `events`, so ties are broken by `rowid` (SQLite's insertion order) and a row carrying no `sequence` is reported as unpositioned rather than placed in the chain;
- each link is compared against the **recomputed** hash of its predecessor, so a mutated event breaks its own link and every later link rather than only its own;
- `payload_json` is hashed as a JSON string containing the stored text, verbatim, rather than re-parsed and re-serialized;
- events whose `project_id` is null form their own chain, keyed by `session_id`;
- verification recomputes the chain from persisted rows and fails at the first mismatch, naming the divergent `event_id` and `sequence`.

The chain is written and checked by `crates/storage`: `append_event` is the only writer of a chain link, so
`prev_hash` and `event_hash` cannot be supplied by a caller, and `recover` reports a break as `EVENT_CHAIN_BROKEN`.

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

## Communication state vocabulary

The communication tables carry four state columns. The canonical schema puts no `CHECK` on any of them, and no
`CHECK` can be added to a table that already exists because the schema is applied with `CREATE TABLE IF NOT
EXISTS` and there is no migration runner. Their vocabularies therefore live here, and the crate that owns the
table is what enforces them.

| Column | Owner | Vocabulary |
| --- | --- | --- |
| `messages.delivery_state` | `crates/bus` | The `message_delivery` machine's twelve states, declared in `schemas/mcf-v2/transition-types.json`. This is the one column whose vocabulary is machine-readable and gate-adjacent, so it is not restated here. |
| `outbox.dispatch_state` | `crates/bus` | `PENDING` (not yet attempted), `FAILED` (the last attempt failed and a retry is scheduled), `DISPATCHED` (handed to the transport), `ABANDONED` (expired; it will not be retried). `PENDING` and `FAILED` are claimable; the other two are terminal (DEC-058) |
| `inbox.processing_state` | `crates/bus` | `PERSISTED` (seen and recorded, not yet acknowledged), `ACKED` (receipt acknowledged), `PROCESSING`, `PROCESSED`, `RETRYING` (refused retryably, awaiting requeue), `DEAD_LETTER` (refused terminally) (DEC-059) |
| `message_receipts.receipt_state` | `crates/bus` | `ACKED`, `NACKED`. A message that was acknowledged and later refused has one row whose state becomes `NACKED`; `acknowledged_at` still records that it was acknowledged, so the two facts do not overwrite each other (DEC-059) |

`inbox.processing_state` has no `DUPLICATE` value, and that is deliberate: a redelivery is recognised by the
existence of the row rather than by a state the row is moved into, so the state continues to describe where the
message actually got to. `inbox.terminal_event_id` holds the event that ended the message's processing -
`ACTION_COMPLETED` or `MESSAGE_DEAD_LETTERED` - which is what makes a redelivery able to return the **prior
outcome** rather than merely a state name.

`message_receipts` declares no uniqueness on `message_id`, so `receipt_id` is derived as `rcpt_{message_id}`:
the derivation is what makes the one-to-one relation true rather than merely intended, and it is what lets the
acknowledgement be updated to a non-acknowledgement without a second row appearing.

`messages.delivery_state` is the state of the **message**, and both directions advance the same column: the
sender's part of the machine runs `CREATED` through `DISPATCHED`, and the receiver's part runs `DISPATCHED`
through `PROCESSED` or `DEAD_LETTER`. A message that arrived from a peer is therefore born `DISPATCHED` - not
by a transition this repository performs, but because its sender dispatched it - and the receiver's own
transitions begin at `RECEIVED`.

`outbox.next_attempt_at` is set to the message's `created_at` on enqueue, normalized to a fixed-width UTC stamp
through SQLite, and thereafter to the instant the last attempt finished plus the policy's backoff. Both the
normalization and the backoff arithmetic are done in SQL rather than in Rust, so the column holds exactly one
spelling of time and the due check is a comparison of like with like. `outbox.attempts` starts at `0` and is the
**durable** attempt count the dispatch decision reads, which is what stops a restart from resetting a message's
retry budget. The `UNIQUE` constraint on `outbox.message_id` makes the queue entry one-to-one with its message,
which is why the bus derives the queue entry's id from the message id rather than minting one.

`message_attempts` records delivery attempts, so a message that exhausts its budget has exactly as many rows as
it made attempts and no row for the expiry itself: the expiry is the decision that trying is over, not a try.
`dead_letters.final_error_json` therefore holds the last recorded failure rather than the expiry, and
`dead_letters.attempts` holds the count that `outbox.attempts` agrees with.

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
project_id, status, phase, epoch, session_id, task_id, lease expiry, message delivery state, retry time, the
idempotency scope (`project_id` with the envelope's `operation_id`, which has no column of its own and is reached
through `json_extract`), the priority lane (also reached through `json_extract`, because the priority belongs to
the envelope and a second copy in a column could disagree with it), event sequence, correlation_id, artifact
hash, validation status and failure fingerprint, council round/phase, barrier status, trace source/target and
coverage status.

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
