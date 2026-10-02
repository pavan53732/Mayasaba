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

## Coverage
NOT_ADDRESSED -> ADDRESSED -> IMPLEMENTED -> VALIDATED -> CERTIFIED

Implemented does not mean validated.

## Orphan detection
Detect requirements without tasks, tasks without requirements or explicit rationale, implementations without a valid task lease, changed files without an owning task, validations without scoped requirements, and certification claims without required evidence.

## Change impact
Changes to requirements, decisions, or contracts identify affected tasks, contexts, validations, and evidence and advance the project epoch according to the canonical context rules.

## Authority
Traceability indexes relationships between authoritative objects. It is not a second source of truth.


## Persistence implementation

Traceability is persisted by the SQLite storage owner.

Canonical tables:
- `trace_links` — typed source → target relationship.
- `trace_link_versions` — immutable supersession/history.
- `trace_coverage` — materialized coverage facts derived from authoritative links and validation/certification state.

Canonical link types:
`INTENT_REQUIREMENT`, `REQUIREMENT_ACCEPTANCE`, `REQUIREMENT_DECISION`, `DECISION_ARCHITECTURE`, `ARCHITECTURE_CONTRACT`, `CONTRACT_TASK`, `TASK_LEASE`, `LEASE_CHANGESET`, `CHANGESET_EXECUTION`, `EXECUTION_EVIDENCE`, `EVIDENCE_REVIEW`, `REVIEW_VALIDATION`, `VALIDATION_CERTIFICATION`.

Orphan detection is a deterministic SQLite query/service operation, not an LLM judgment.

The canonical admission query is: any integrated changeset whose latest `INTEGRATION_ADMISSION` verdict is not `ADMITTED`, or which has no admission record at all, is an orphan and blocks certification.