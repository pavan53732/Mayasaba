# Mayasaba Requirements-to-Evidence Traceability

## Purpose
Every material requirement must be traceable through design, implementation, validation, and certification.

## Canonical chain
USER INTENT -> REQUIREMENT -> ACCEPTANCE CRITERIA -> DECISION / HARD_LOCK -> ARCHITECTURE / CONTRACT -> TASK -> TASK LEASE / AGENT -> CHANGESET / ARTIFACT -> TASK-APPROPRIATE CHECKS (EXECUTION / TEST / SOURCE REVIEW / INTEGRITY) -> EVIDENCE -> REVIEW -> VALIDATION -> CERTIFICATION

## Intent anchor
USER INTENT is the `ProjectBrief` version that was current when DISCOVERY closed. The existing `INTENT_REQUIREMENT` link type connects that brief version to the requirements derived from it. Because brief versions are immutable, the anchor for a lineage is stable: a later brief version starts a new derivation path rather than rewriting the links of an earlier one.

## Traceability references
Requirements may link to acceptance criteria, decisions, architecture artifacts, contracts, tasks, commits/changesets, executions, tests, evidence, reviews, and validation runs.

Admission records are traceable objects: a `WORKSPACE_ADMISSION` links a task lease to the workspace it was admitted into, and an `INTEGRATION_ADMISSION` links a changeset to the integration decision that let it through. Because the record carries its own evidence and verdict, "changed files without an owning task" and "integrations without an admission" are both detectable.

Decision-quality records — mode selections, claim grades, round roles, budget ledger entries and decision outcomes — attach to the existing canonical chain at council round → decision → validation. They add no new trace link type: the canonical link-type vocabulary is closed, and a new type would require its own decision.

## Coverage
NOT_ADDRESSED -> ADDRESSED -> IMPLEMENTED -> VALIDATED -> CERTIFIED

Implemented does not mean validated.

## Orphan detection
Detect requirements without tasks, tasks without requirements or explicit rationale, implementations without a valid task lease, changed files without an owning task, validations without scoped requirements, and certification claims without required evidence.

## Change impact
Changes to requirements, decisions, or contracts identify affected tasks, contexts, validations, and evidence and advance the project epoch according to the canonical context rules.

## Authority
Traceability indexes relationships between authoritative objects. It is not a second source of truth.

## M2 communication bus coverage

The M2 slice (`crates/bus`, with its `crates/storage` support) is **IMPLEMENTED and VALIDATED**, not CERTIFIED:
certification is the controller's, and no controller has certified it. Each requirement below reaches VALIDATED
through the tests named, and each decision is a record in `docs/DECISION-REGISTER.md`.

| Requirement | Decision | Evidence |
| --- | --- | --- |
| Durable event log with a per-project hash chain | DEC-034 | `crates/storage/tests/event_chain.rs`, `canonical.rs` |
| Transactional outbox, envelope-gated, idempotent | DEC-058, DEC-065 | `crates/bus/tests/enqueue.rs` |
| Pure dispatch decision, policy file, bounded retry, backoff, dead letters | DEC-058 | `crates/bus/tests/dispatch.rs` |
| Durable inbox, persist-before-ACK, receipt-only ACK, NACK reason, duplicate delivery | DEC-059 | `crates/bus/tests/inbox.rs` |
| Declared priority lanes in the contract's order | DEC-060 | `crates/bus/tests/dispatch.rs`, gate check |
| Requeue, and event identity across a repeated transition | DEC-061 | `crates/bus/tests/inbox.rs` |
| `RETRYING -> EXPIRED`, closing the last declared exit | DEC-062 | `crates/bus/tests/inbox.rs` |
| Ordering gap detection | DEC-063 | `crates/bus/tests/inbox.rs` |
| Backpressure as measurement | DEC-064 | `crates/bus/tests/dispatch.rs` |
| Explicit controller-invoked replay | DEC-065 | `crates/bus/tests/replay.rs` |
| The delivery code held to the declared machine | DEC-066 | `npm run verify:contracts`, mutation `dec066-a` |

**Not addressed, and recorded as such rather than implied.** Backpressure does not refuse an enqueue, because the
error registry has no code for transient capacity (DEC-064); no production clock, transport or identity source is
shipped, because the bus performs no I/O (DEC-058, DEC-065); four of the machine's thirteen edges are not read by
the gate, because they are inline SQL where the from-state is implied by a `WHERE` clause (DEC-066). The authorization decision is persisted as the canonical `AUTHORIZATION_VALIDATED` event in the immutable event chain (owned by `PolicyService`) rather than as an outbox table column, resolving that gap.


## Persistence implementation

Traceability is persisted by the SQLite storage owner.

Canonical tables:
- `trace_links` — typed source → target relationship.
- `trace_link_versions` — immutable supersession/history.
- `trace_coverage` — materialized coverage facts derived from authoritative links and validation/certification state.

A trace link is validated by `schemas/trace-v1/trace-link.schema.json`; its `link_type` is the closed vocabulary of the canonical chain above.

Canonical link types:
`INTENT_REQUIREMENT`, `REQUIREMENT_ACCEPTANCE`, `REQUIREMENT_DECISION`, `DECISION_ARCHITECTURE`, `ARCHITECTURE_CONTRACT`, `CONTRACT_TASK`, `TASK_LEASE`, `LEASE_CHANGESET`, `CHANGESET_EXECUTION`, `EXECUTION_EVIDENCE`, `EVIDENCE_REVIEW`, `REVIEW_VALIDATION`, `VALIDATION_CERTIFICATION`.

Orphan detection is a deterministic SQLite query/service operation, not an LLM judgment.

The canonical admission query is: any integrated changeset whose latest `INTEGRATION_ADMISSION` verdict is not `ADMITTED`, or which has no admission record at all, is an orphan and blocks certification.